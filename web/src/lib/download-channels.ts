/**
 * What counts as a SoloMD download, shared by the build-time Hero numbers,
 * /api/stats and the admin dashboard so every place agrees.
 *
 * The public total is: GitHub installers + App Store + Microsoft Store +
 * Google Play. Homebrew / winget / Scoop / Chocolatey / AUR all download the
 * installer from GitHub, so they are already inside the GitHub number and are
 * never added on top (the dashboard shows Chocolatey's own counter for
 * reference only).
 *
 * GitHub counts include updates — the in-app updater fetches the same files —
 * so it is "downloads", not "new users". The App Store figure is first-time
 * downloads only (Apple reports updates separately and we leave them out).
 */

export type Platform = 'windows' | 'macos' | 'linux' | 'android';
export type AssetKind = Platform | 'tools' | null;

/** Classify a release asset by file name. `null` = not a download we count
 *  (checksums, the Play-only .aab, one-off test builds). */
export function classifyAsset(name: string): AssetKind {
  const n = name.toLowerCase();
  if (n.startsWith('solomd-mcp') || n.startsWith('solomd-clipper') || n.startsWith('solomd-skills')) {
    return 'tools';
  }
  if (n.includes('sha256sums') || n.endsWith('.sig') || n.endsWith('.json') || n.endsWith('.txt')) return null;
  if (n.includes('-test.')) return null; // e.g. the ubuntu2404-test diagnostic build
  if (n.endsWith('.aab')) return null; // uploaded to Play; nobody installs it from GitHub
  if (n.endsWith('.dmg')) return 'macos';
  if (n.endsWith('.msi') || n.endsWith('.exe') || n.endsWith('-portable.zip')) return 'windows';
  if (n.endsWith('.apk')) return 'android';
  if (n.endsWith('.appimage') || n.endsWith('.deb') || n.endsWith('.rpm')) return 'linux';
  return null;
}

export interface GithubDownloads {
  /** Installers across all platforms — the part of the public total. */
  installers: number;
  byPlatform: Record<Platform, number>;
  /** MCP CLI, web clipper, skills packs. Shown in the dashboard only. */
  tools: number;
  stars: number | null;
  latestTag: string | null;
  latestUrl: string | null;
  releases: number;
}

interface GhRelease {
  tag_name?: string;
  html_url?: string;
  draft?: boolean;
  prerelease?: boolean;
  assets?: Array<{ name?: string; download_count?: number }>;
}

/**
 * Every release, all pages. The old code read only `per_page=100`, which
 * silently dropped the oldest releases once there were more than 100.
 * Returns null when GitHub didn't answer properly, so callers fall back to
 * a cached value instead of publishing a wrong number.
 */
export async function fetchGithubDownloads(
  repo: string,
  headers: Record<string, string>,
): Promise<GithubDownloads | null> {
  const repoRes = await fetch(`https://api.github.com/repos/${repo}`, { headers });
  const stars = repoRes.ok
    ? ((await repoRes.json()) as { stargazers_count?: number }).stargazers_count ?? null
    : null;

  const all: GhRelease[] = [];
  for (let page = 1; page <= 20; page++) {
    const res = await fetch(`https://api.github.com/repos/${repo}/releases?per_page=100&page=${page}`, {
      headers,
    });
    if (!res.ok) return null;
    const batch = (await res.json()) as GhRelease[];
    if (!Array.isArray(batch)) return null;
    all.push(...batch);
    if (batch.length < 100) break;
  }
  // An empty list is never a real answer for this repo (see /api/stats).
  if (all.length === 0) return null;

  const byPlatform: Record<Platform, number> = { windows: 0, macos: 0, linux: 0, android: 0 };
  let tools = 0;
  for (const rel of all) {
    if (rel.draft) continue;
    for (const a of rel.assets || []) {
      const kind = classifyAsset(a.name || '');
      const c = a.download_count || 0;
      if (kind === 'tools') tools += c;
      else if (kind) byPlatform[kind] += c;
    }
  }
  const installers = byPlatform.windows + byPlatform.macos + byPlatform.linux + byPlatform.android;
  if (installers === 0) return null;

  const stable = all.find((r) => !r.draft && !r.prerelease);
  return {
    installers,
    byPlatform,
    tools,
    stars,
    latestTag: stable ? (stable.tag_name || '').replace(/^v/, '') || null : null,
    latestUrl: stable?.html_url || null,
    releases: all.length,
  };
}

/** Channels whose numbers are collected daily and stored in D1. */
export const STORE_CHANNELS = ['appstore', 'msstore', 'play'] as const;
export type StoreChannel = (typeof STORE_CHANNELS)[number];
