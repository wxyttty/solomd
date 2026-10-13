/**
 * Cloudflare Pages Function: /api/stats
 *
 * Server-side proxy for GitHub stats. Fetches stars + total download count,
 * caches for 5 minutes in Cloudflare's edge cache. This means:
 *   - Clients are never rate-limited (they only hit our own domain)
 *   - GitHub API gets at most 1 req / 5 min per edge location (far below
 *     the 60/hour anon limit, or 5000/hour authenticated)
 *   - Stats update automatically every 5 minutes without any redeploy
 *
 * Resilience layers (added 2026-05-04 after homepage showed empty downloads
 * for everyone for ~24h while GitHub anon rate-limit was burned):
 *   1. If GITHUB_TOKEN env is set, send Bearer auth → 5000/hr ceiling.
 *   2. Successful responses are written to D1 (`stats_cache` row id='github')
 *      so we have last-known-good values to serve when GitHub is unreachable.
 *   3. On fetch failure, fall back to the D1 cached values rather than null.
 *      The visitor still sees real numbers; they just may be a few hours stale.
 *
 * Response: { stars, downloads, github_downloads, stores, updated, latest_tag,
 *             latest_url, fresh }
 *   - `downloads` is the all-channel total shown on the homepage: GitHub
 *     installers + App Store + Microsoft Store + Google Play (see
 *     src/lib/download-channels.ts for what counts and why package managers
 *     aren't added on top). Store figures come from D1 (download_daily),
 *     written once a day by the download-stats workflow.
 *   - `github_downloads` is the GitHub installer part alone.
 *   - `fresh: true` means the values came from a successful upstream fetch.
 *   - `fresh: false` means they came from the D1 fallback cache.
 *
 * Desktop app's "Check for updates" feature also calls this endpoint
 * (instead of api.github.com directly) to avoid hitting GitHub's
 * 60 req/hour unauth rate limit on the user's own IP.
 */

import { fetchGithubDownloads, STORE_CHANNELS } from '../../src/lib/download-channels';

const REPO = 'zhitongblog/solomd';
const CACHE_TTL = 300; // 5 minutes (edge cache TTL on success)
const CACHE_TTL_STALE = 60; // 1 minute (when serving D1 fallback — refresh sooner)

/** The GitHub part, cached in D1 as last-known-good. */
interface GithubPayload {
  stars: number | null;
  /** GitHub installers only (kept under the old name in the cache row). */
  downloads: number | null;
  latest_tag: string | null;
  latest_url: string | null;
}

interface StatsEnv {
  DB?: D1Database;
  GITHUB_TOKEN?: string;
}

export const onRequest: PagesFunction<StatsEnv> = async ({ request, env }) => {
  const cache = (caches as any).default as Cache;
  const cacheKey = new Request(request.url, request);

  // 1. Edge cache short-circuit
  const cached = await cache.match(cacheKey);
  if (cached) return cached;

  // 2. Try GitHub (with token if available)
  let payload: GithubPayload = { stars: null, downloads: null, latest_tag: null, latest_url: null };
  let fetchOk = false;
  try {
    const headers: Record<string, string> = {
      'User-Agent': 'SoloMD-stats-proxy',
      Accept: 'application/vnd.github+json',
    };
    if (env.GITHUB_TOKEN) headers.Authorization = `Bearer ${env.GITHUB_TOKEN}`;
    // Returns null on any upstream failure, an empty release list or a zero
    // installer sum — never a wrong number (see the 2026-08-17 note in git
    // history: a "fresh" 0 once overwrote the last-known-good cache).
    const gh = await fetchGithubDownloads(REPO, headers);
    if (gh) {
      payload = {
        stars: gh.stars,
        downloads: gh.installers,
        latest_tag: gh.latestTag,
        latest_url: gh.latestUrl,
      };
      fetchOk = gh.stars != null;
    }
  } catch {
    // Network blip; fall through to the D1 fallback.
  }

  // 3. Persist on success / read on failure
  if (fetchOk && env.DB) {
    try {
      await env.DB.prepare(
        `INSERT INTO stats_cache(key, value, updated_at) VALUES(?, ?, ?)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at`,
      )
        .bind('github', JSON.stringify(payload), Date.now())
        .run();
    } catch {
      // Cache-write failure is non-fatal; the response is still served.
    }
  } else if (env.DB) {
    try {
      const row = await env.DB.prepare(`SELECT value FROM stats_cache WHERE key = ?`)
        .bind('github')
        .first<{ value: string }>();
      if (row?.value) {
        const fallback = JSON.parse(row.value) as GithubPayload;
        // Prefer whatever did come back fresh, cached values for the rest.
        payload = {
          stars: payload.stars ?? fallback.stars,
          downloads: payload.downloads ?? fallback.downloads,
          latest_tag: payload.latest_tag ?? fallback.latest_tag,
          latest_url: payload.latest_url ?? fallback.latest_url,
        };
      }
    } catch {
      // D1 unreachable — return whatever we got from GitHub (possibly all-null).
    }
  }

  // 4. Store channels (written daily by the collector). Missing table or
  // no rows yet → the total is simply the GitHub part.
  const stores: Record<string, number | null> = {};
  for (const ch of STORE_CHANNELS) stores[ch] = null;
  if (env.DB) {
    try {
      const rows = await env.DB.prepare(
        `SELECT channel, SUM(count) AS total FROM download_daily GROUP BY channel`,
      ).all<{ channel: string; total: number }>();
      for (const r of rows.results) if (r.channel in stores) stores[r.channel] = r.total;
    } catch {
      // Table not migrated yet, or D1 blip — show the GitHub part alone.
    }
  }
  const storeSum = Object.values(stores).reduce<number>((a, b) => a + (b ?? 0), 0);
  const total = payload.downloads != null ? payload.downloads + storeSum : null;

  const body = JSON.stringify({
    stars: payload.stars,
    downloads: total,
    github_downloads: payload.downloads,
    stores,
    latest_tag: payload.latest_tag,
    latest_url: payload.latest_url,
    updated: new Date().toISOString(),
    fresh: fetchOk,
  });

  // Cache fresh responses for the full TTL; stale-fallback responses only
  // briefly so the next request retries upstream sooner.
  const ttl = fetchOk ? CACHE_TTL : CACHE_TTL_STALE;

  const response = new Response(body, {
    headers: {
      'Content-Type': 'application/json; charset=utf-8',
      'Cache-Control': `public, max-age=${ttl}, s-maxage=${ttl}`,
      'Access-Control-Allow-Origin': '*',
    },
  });

  if (fetchOk) {
    await cache.put(cacheKey, response.clone());
  }

  return response;
};
