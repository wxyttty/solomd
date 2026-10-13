/**
 * /api/admin/downloads — all-channel download counts.
 *
 * POST  Authorization: Bearer <INGEST_TOKEN>
 *   The daily collector (.github/workflows/download-stats.yml) writes here.
 *   Body: {
 *     daily?:  [{ channel: 'appstore'|'msstore'|'play', day: 'YYYY-MM-DD', count }],
 *     totals?: [{ channel: 'github'|'chocolatey', day: 'YYYY-MM-DD', total, detail? }],
 *   }
 *   Upserts, so re-sending a day is harmless. A separate token from
 *   ADMIN_TOKEN: the collector can write numbers but cannot read the
 *   analytics dashboard, and rotating one doesn't break the other.
 *
 * GET   Authorization: Bearer <ADMIN_TOKEN>
 *   Everything the dashboard needs: per-channel totals, the public total,
 *   GitHub's per-platform split and a per-day series for the last 120 days.
 */

import { fetchGithubDownloads, STORE_CHANNELS } from '../../../src/lib/download-channels';

interface Env {
  DB: D1Database;
  ADMIN_TOKEN?: string;
  INGEST_TOKEN?: string;
  GITHUB_TOKEN?: string;
}

const REPO = 'zhitongblog/solomd';
const DAILY_CHANNELS = new Set<string>(STORE_CHANNELS);
const TOTAL_CHANNELS = new Set(['github', 'chocolatey']);
const DAY_RE = /^\d{4}-\d{2}-\d{2}$/;
const MAX_COUNT = 50_000_000;
const MAX_ROWS = 2000;

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json; charset=utf-8', 'Cache-Control': 'no-store' },
  });
}

function bearerOk(request: Request, expected: string | undefined): boolean {
  if (!expected) return false;
  const m = (request.headers.get('authorization') || '').match(/^Bearer\s+(.+)$/i);
  if (!m) return false;
  const got = m[1];
  if (got.length !== expected.length) return false;
  let diff = 0;
  for (let i = 0; i < got.length; i += 1) diff |= got.charCodeAt(i) ^ expected.charCodeAt(i);
  return diff === 0;
}

function validCount(n: unknown): n is number {
  return typeof n === 'number' && Number.isInteger(n) && n >= 0 && n <= MAX_COUNT;
}

export const onRequestPost: PagesFunction<Env> = async ({ request, env }) => {
  if (!bearerOk(request, env.INGEST_TOKEN)) return json({ error: 'unauthorized' }, 401);
  let body: { daily?: unknown; totals?: unknown };
  try {
    body = await request.json();
  } catch {
    return json({ error: 'bad json' }, 400);
  }
  const daily = Array.isArray(body.daily) ? body.daily : [];
  const totals = Array.isArray(body.totals) ? body.totals : [];
  if (daily.length + totals.length > MAX_ROWS) return json({ error: 'too many rows' }, 413);

  const now = Date.now();
  const stmts: D1PreparedStatement[] = [];
  const rejected: unknown[] = [];

  for (const r of daily as Array<Record<string, unknown>>) {
    if (!r || !DAILY_CHANNELS.has(String(r.channel)) || !DAY_RE.test(String(r.day)) || !validCount(r.count)) {
      rejected.push(r);
      continue;
    }
    stmts.push(
      env.DB.prepare(
        `INSERT INTO download_daily(channel, day, count, updated_at) VALUES(?, ?, ?, ?)
         ON CONFLICT(channel, day) DO UPDATE SET count=excluded.count, updated_at=excluded.updated_at`,
      ).bind(String(r.channel), String(r.day), r.count, now),
    );
  }
  for (const r of totals as Array<Record<string, unknown>>) {
    if (!r || !TOTAL_CHANNELS.has(String(r.channel)) || !DAY_RE.test(String(r.day)) || !validCount(r.total)) {
      rejected.push(r);
      continue;
    }
    const detail = r.detail && typeof r.detail === 'object' ? JSON.stringify(r.detail).slice(0, 2000) : '{}';
    stmts.push(
      env.DB.prepare(
        `INSERT INTO download_totals(channel, day, total, detail, updated_at) VALUES(?, ?, ?, ?, ?)
         ON CONFLICT(channel, day) DO UPDATE SET total=excluded.total, detail=excluded.detail, updated_at=excluded.updated_at`,
      ).bind(String(r.channel), String(r.day), r.total, detail, now),
    );
  }
  if (stmts.length) await env.DB.batch(stmts);
  return json({ ok: true, written: stmts.length, rejected: rejected.length });
};

export const onRequestGet: PagesFunction<Env> = async ({ request, env }) => {
  if (!bearerOk(request, env.ADMIN_TOKEN)) return json({ error: 'unauthorized' }, 401);

  const storeSums = await env.DB.prepare(
    `SELECT channel, SUM(count) AS total, MIN(day) AS first_day, MAX(day) AS last_day, MAX(updated_at) AS updated_at
     FROM download_daily GROUP BY channel`,
  ).all<{ channel: string; total: number; first_day: string; last_day: string; updated_at: number }>();

  const latestTotals = await env.DB.prepare(
    `SELECT t.channel, t.day, t.total, t.detail, t.updated_at FROM download_totals t
     JOIN (SELECT channel, MAX(day) AS day FROM download_totals GROUP BY channel) m
       ON m.channel = t.channel AND m.day = t.day`,
  ).all<{ channel: string; day: string; total: number; detail: string; updated_at: number }>();

  const since = new Date(Date.now() - 120 * 86_400_000).toISOString().slice(0, 10);
  const storeDays = await env.DB.prepare(
    `SELECT channel, day, count FROM download_daily WHERE day >= ? ORDER BY day`,
  ).bind(since).all<{ channel: string; day: string; count: number }>();
  // One extra day before the window so the first GitHub difference exists.
  const ghDays = await env.DB.prepare(
    `SELECT day, total FROM download_totals WHERE channel = 'github' AND day >= date(?, '-1 day') ORDER BY day`,
  ).bind(since).all<{ day: string; total: number }>();

  // GitHub live (the stored snapshot is at most a day old; the dashboard
  // shows the live figure and falls back to the snapshot).
  const headers: Record<string, string> = { 'User-Agent': 'SoloMD-admin', Accept: 'application/vnd.github+json' };
  if (env.GITHUB_TOKEN) headers.Authorization = `Bearer ${env.GITHUB_TOKEN}`;
  let github = null as Awaited<ReturnType<typeof fetchGithubDownloads>>;
  try {
    github = await fetchGithubDownloads(REPO, headers);
  } catch {
    github = null;
  }
  const ghSnap = latestTotals.results.find((r) => r.channel === 'github');
  const ghTotal = github?.installers ?? ghSnap?.total ?? 0;

  const channels: Record<string, unknown> = {
    github: {
      total: ghTotal,
      live: !!github,
      byPlatform: github?.byPlatform ?? (ghSnap ? JSON.parse(ghSnap.detail || '{}').byPlatform ?? null : null),
      tools: github?.tools ?? (ghSnap ? JSON.parse(ghSnap.detail || '{}').tools ?? null : null),
    },
  };
  let publicTotal = ghTotal;
  for (const ch of STORE_CHANNELS) {
    const row = storeSums.results.find((r) => r.channel === ch);
    channels[ch] = row
      ? { total: row.total, first_day: row.first_day, last_day: row.last_day, updated_at: row.updated_at }
      : { total: null };
    publicTotal += row?.total ?? 0;
  }
  const choco = latestTotals.results.find((r) => r.channel === 'chocolatey');
  channels.chocolatey = choco ? { total: choco.total, day: choco.day, counted: false } : { total: null, counted: false };

  // Per-day series: store channels as reported, GitHub as day-over-day delta.
  const series: Record<string, Record<string, number>> = {};
  for (const r of storeDays.results) (series[r.day] ??= {})[r.channel] = r.count;
  for (let i = 1; i < ghDays.results.length; i++) {
    const d = ghDays.results[i];
    const delta = d.total - ghDays.results[i - 1].total;
    if (delta >= 0 && d.day >= since) (series[d.day] ??= {}).github = delta;
  }
  const daily = Object.keys(series)
    .sort()
    .map((day) => ({ day, ...series[day] }));

  return json({ publicTotal, channels, daily, generatedAt: new Date().toISOString() });
};
