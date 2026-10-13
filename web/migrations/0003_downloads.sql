-- All-channel download counts, written once a day by
-- .github/workflows/download-stats.yml (scripts/collect-downloads.py) through
-- POST /api/admin/downloads, read by /api/stats (public total) and
-- GET /api/admin/downloads (dashboard).
--
-- Two shapes, because the sources report two ways:
--   * download_daily  — a count per calendar day. App Store, Microsoft Store
--     and Google Play all report per day; a channel's total is SUM(count).
--     Rows are upserted, so re-collecting a day that the store revised is
--     harmless, and history survives even if the store later stops serving
--     old report files.
--   * download_totals — a cumulative figure as of a day. GitHub and
--     Chocolatey only expose a running total; one snapshot per day gives the
--     daily trend as the difference between consecutive days.

CREATE TABLE IF NOT EXISTS download_daily (
  channel TEXT NOT NULL,        -- 'appstore' | 'msstore' | 'play'
  day TEXT NOT NULL,            -- YYYY-MM-DD (the day the downloads happened)
  count INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,  -- ms since epoch
  PRIMARY KEY (channel, day)
);

CREATE TABLE IF NOT EXISTS download_totals (
  channel TEXT NOT NULL,        -- 'github' | 'chocolatey'
  day TEXT NOT NULL,            -- YYYY-MM-DD (UTC day of the snapshot)
  total INTEGER NOT NULL,
  detail TEXT NOT NULL DEFAULT '{}', -- JSON, e.g. per-platform split
  updated_at INTEGER NOT NULL,
  PRIMARY KEY (channel, day)
);
