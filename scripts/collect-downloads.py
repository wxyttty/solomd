#!/usr/bin/env python3
"""Collect download counts from every channel and send them to solomd.app.

Runs once a day from .github/workflows/download-stats.yml; also runnable by
hand:

    python3 scripts/collect-downloads.py --dry-run     # print, send nothing
    python3 scripts/collect-downloads.py               # POST to solomd.app

What each channel reports, and how it is stored (web/migrations/0003):

  github      cumulative installer downloads (all release pages), one
              snapshot per UTC day -> download_totals. Classification must
              match web/src/lib/download-channels.ts::classifyAsset.
  chocolatey  the package's own counter -> download_totals. Dashboard only:
              the package fetches the MSI from GitHub, so it is already in
              the GitHub number and is never added to the public total.
  appstore    first-time downloads per day (iPhone, iPad and Mac) from the
              App Store Connect "App Downloads Standard" analytics report ->
              download_daily. Apple re-sends recent days in later daily
              files; the newest file wins for a given date (checked equal).
  msstore     acquisitions per day, Microsoft Store analytics API ->
              download_daily. Needs MSSTORE_TENANT_ID / MSSTORE_CLIENT_ID /
              MSSTORE_CLIENT_SECRET (an Azure AD app associated with the
              Partner Center account).
  play        "Daily User Installs" from the monthly installs CSVs Google Play
              exports to Cloud Storage -> download_daily. Needs
              PLAY_BUCKET (pubsite_prod_…) and PLAY_SERVICE_ACCOUNT_JSON.

A channel without credentials is skipped with a note, never sent as zero.

Environment:
  GITHUB_TOKEN                     optional (rate limit)
  ASC_KEY_ID, ASC_ISSUER_ID and ASC_KEY_PATH or ASC_KEY_P8 (key contents)
  SOLOMD_INGEST_TOKEN              required unless --dry-run
  SOLOMD_INGEST_URL                default https://solomd.app/api/admin/downloads

Standard library + the openssl binary only, like scripts/lib/asc_api.py.
"""

import argparse
import base64
import collections
import csv
import datetime as dt
import gzip
import io
import json
import os
import re
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "lib"))

REPO = "zhitongblog/solomd"
BUNDLE_ID = "app.solomd"
MSSTORE_APP_ID = "9NXGHK2LL1Q4"
PLAY_PACKAGE = "app.solomd"
UA = "SoloMD-download-stats"

# Direct connections: the dev machine's proxy drops Apple/Google calls (see
# scripts/lib/asc_api.py). CI has no proxy either way.
_opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def http(url, data=None, headers=None, method=None, timeout=60):
    h = {"User-Agent": UA}
    h.update(headers or {})
    req = urllib.request.Request(url, data=data, headers=h, method=method)
    last = None
    for attempt in range(3):
        try:
            with _opener.open(req, timeout=timeout) as r:
                return r.read()
        except urllib.error.HTTPError as e:
            if e.code < 500:
                raise RuntimeError(f"{method or 'GET'} {url.split('?')[0]} -> {e.code}: {e.read()[:300]!r}")
            last = e
        except Exception as e:  # network blip
            last = e
        time.sleep(2 * (attempt + 1))
    raise RuntimeError(f"{url.split('?')[0]}: {last}")


def log(*a):
    print(*a, file=sys.stderr, flush=True)


# ---------------------------------------------------------------- GitHub

def classify(name: str):
    """Keep in sync with web/src/lib/download-channels.ts::classifyAsset."""
    n = name.lower()
    if n.startswith(("solomd-mcp", "solomd-clipper", "solomd-skills")):
        return "tools"
    if "sha256sums" in n or n.endswith((".sig", ".json", ".txt")):
        return None
    if "-test." in n or n.endswith(".aab"):
        return None
    if n.endswith(".dmg"):
        return "macos"
    if n.endswith((".msi", ".exe", "-portable.zip")):
        return "windows"
    if n.endswith(".apk"):
        return "android"
    if n.endswith((".appimage", ".deb", ".rpm")):
        return "linux"
    return None


def collect_github():
    headers = {"Accept": "application/vnd.github+json"}
    if os.environ.get("GITHUB_TOKEN"):
        headers["Authorization"] = "Bearer " + os.environ["GITHUB_TOKEN"]
    releases = []
    for page in range(1, 21):
        batch = json.loads(http(f"https://api.github.com/repos/{REPO}/releases?per_page=100&page={page}",
                                headers=headers))
        releases += batch
        if len(batch) < 100:
            break
    by = {"windows": 0, "macos": 0, "linux": 0, "android": 0}
    tools = 0
    for rel in releases:
        if rel.get("draft"):
            continue
        for a in rel.get("assets", []):
            k = classify(a.get("name", ""))
            if k == "tools":
                tools += a.get("download_count", 0)
            elif k:
                by[k] += a.get("download_count", 0)
    total = sum(by.values())
    if not releases or total == 0:
        raise RuntimeError("GitHub returned no releases / zero installers — refusing to record")
    return total, {"byPlatform": by, "tools": tools, "releases": len(releases)}


# ---------------------------------------------------------------- Chocolatey

def collect_chocolatey():
    # The package-level DownloadCount field reads 0 on community.chocolatey.org;
    # the real figures are per version (VersionDownloadCount), so sum those.
    url = "https://community.chocolatey.org/api/v2/FindPackagesById()?" + urllib.parse.urlencode({"id": "'solomd'"})
    total, versions = 0, 0
    while url:
        xml = http(url).decode("utf-8", "replace")
        for m in re.finditer(r"<d:VersionDownloadCount[^>]*>(\d+)</d:VersionDownloadCount>", xml):
            total += int(m.group(1))
            versions += 1
        nxt = re.search(r'<link rel="next" href="([^"]+)"', xml)
        url = nxt.group(1).replace("&amp;", "&") if nxt else None
    if versions == 0:
        raise RuntimeError("Chocolatey: no versions found")
    return total


# ---------------------------------------------------------------- App Store

def _asc_client():
    from asc_api import Client, make_token
    key_id, issuer = os.environ.get("ASC_KEY_ID"), os.environ.get("ASC_ISSUER_ID")
    key_path = os.environ.get("ASC_KEY_PATH")
    if key_path:
        key_path = os.path.expanduser(os.path.expandvars(key_path))
    if not key_path and os.environ.get("ASC_KEY_P8"):
        fd, key_path = tempfile.mkstemp(suffix=".p8")
        with os.fdopen(fd, "w") as f:
            f.write(os.environ["ASC_KEY_P8"])
    if not (key_id and issuer and key_path):
        return None
    return Client(make_token(key_path, key_id, issuer))


def _asc_all(c, path, **params):
    out, res = [], c.get(path, **params)
    while True:
        out += res.get("data", [])
        nxt = (res.get("links") or {}).get("next")
        if not nxt:
            return out
        res = c.get(nxt.replace("https://api.appstoreconnect.apple.com", ""))


def collect_appstore():
    c = _asc_client()
    if c is None:
        return None, "no ASC_KEY_ID / ASC_ISSUER_ID / key"
    app = c.app_id(BUNDLE_ID)
    per_day = {}          # date -> (processingDate, count) ; newest processing wins
    files = 0
    for req in _asc_all(c, f"/v1/apps/{app}/analyticsReportRequests"):
        for rep in _asc_all(c, f"/v1/analyticsReportRequests/{req['id']}/reports", limit=200):
            if rep["attributes"].get("name") != "App Downloads Standard":
                continue
            for inst in _asc_all(c, f"/v1/analyticsReports/{rep['id']}/instances", limit=200):
                if inst["attributes"].get("granularity") != "DAILY":
                    continue
                processed = inst["attributes"].get("processingDate", "")
                counts = collections.Counter()
                for seg in _asc_all(c, f"/v1/analyticsReportInstances/{inst['id']}/segments"):
                    raw = gzip.decompress(http(seg["attributes"]["url"]))
                    files += 1
                    for row in csv.DictReader(io.StringIO(raw.decode("utf-8")), delimiter="\t"):
                        if row.get("Download Type") == "First-time download":
                            counts[row["Date"]] += int(row["Counts"] or 0)
                for day, n in counts.items():
                    if day not in per_day or processed >= per_day[day][0]:
                        per_day[day] = (processed, n)
    if not per_day:
        return None, "no App Downloads Standard data yet"
    log(f"appstore: {files} report files, {len(per_day)} days, "
        f"{sum(n for _, n in per_day.values())} first-time downloads")
    return {d: n for d, (_, n) in per_day.items()}, None


# ---------------------------------------------------------------- Microsoft Store

def collect_msstore():
    tenant, cid, secret = (os.environ.get(k) for k in
                           ("MSSTORE_TENANT_ID", "MSSTORE_CLIENT_ID", "MSSTORE_CLIENT_SECRET"))
    if not (tenant and cid and secret):
        return None, "no MSSTORE_TENANT_ID / MSSTORE_CLIENT_ID / MSSTORE_CLIENT_SECRET"
    tok = json.loads(http(
        f"https://login.microsoftonline.com/{tenant}/oauth2/token",
        data=urllib.parse.urlencode({
            "grant_type": "client_credentials", "client_id": cid, "client_secret": secret,
            "resource": "https://manage.devcenter.microsoft.com",
        }).encode(),
        headers={"Content-Type": "application/x-www-form-urlencoded"}, method="POST"))["access_token"]
    start = "2026-01-01"  # before the first Store listing; the API caps nothing earlier
    end = dt.date.today().isoformat()
    per_day = collections.Counter()
    skip = 0
    while True:
        q = urllib.parse.urlencode({"applicationId": MSSTORE_APP_ID, "startDate": start, "endDate": end,
                                    "aggregationLevel": "day", "top": 10000, "skip": skip})
        page = json.loads(http(f"https://manage.devcenter.microsoft.com/v1.0/my/analytics/appacquisitions?{q}",
                               headers={"Authorization": f"Bearer {tok}"}))
        rows = page.get("Value") or page.get("value") or []
        for r in rows:
            per_day[str(r.get("date", ""))[:10]] += int(r.get("acquisitionQuantity") or 0)
        if not page.get("@nextLink") or not rows:
            break
        skip += len(rows)
    per_day.pop("", None)
    return (dict(per_day) or None), (None if per_day else "API returned no rows")


# ---------------------------------------------------------------- Google Play

def _google_token(sa: dict) -> str:
    now = int(time.time())
    b64 = lambda b: base64.urlsafe_b64encode(b).rstrip(b"=").decode()
    header = b64(json.dumps({"alg": "RS256", "typ": "JWT"}).encode())
    claims = b64(json.dumps({
        "iss": sa["client_email"], "scope": "https://www.googleapis.com/auth/devstorage.read_only",
        "aud": "https://oauth2.googleapis.com/token", "iat": now, "exp": now + 3600,
    }).encode())
    with tempfile.NamedTemporaryFile("w", suffix=".pem", delete=False) as f:
        f.write(sa["private_key"])
        pem = f.name
    try:
        sig = subprocess.run(["openssl", "dgst", "-sha256", "-sign", pem],
                             input=f"{header}.{claims}".encode(), capture_output=True, check=True).stdout
    finally:
        os.unlink(pem)
    assertion = f"{header}.{claims}.{b64(sig)}"
    return json.loads(http("https://oauth2.googleapis.com/token", method="POST",
                           data=urllib.parse.urlencode({
                               "grant_type": "urn:ietf:params:oauth:grant-type:jwt-bearer",
                               "assertion": assertion}).encode(),
                           headers={"Content-Type": "application/x-www-form-urlencoded"}))["access_token"]


def collect_play():
    bucket, sa_json = os.environ.get("PLAY_BUCKET"), os.environ.get("PLAY_SERVICE_ACCOUNT_JSON")
    if not (bucket and sa_json):
        return None, "no PLAY_BUCKET / PLAY_SERVICE_ACCOUNT_JSON"
    tok = _google_token(json.loads(sa_json))
    auth = {"Authorization": f"Bearer {tok}"}
    prefix = f"stats/installs/installs_{PLAY_PACKAGE}_"
    listing = json.loads(http(
        f"https://storage.googleapis.com/storage/v1/b/{bucket}/o?"
        + urllib.parse.urlencode({"prefix": prefix}), headers=auth))
    per_day = {}
    for obj in listing.get("items", []):
        name = obj["name"]
        if not name.endswith("_overview.csv"):
            continue
        raw = http(f"https://storage.googleapis.com/storage/v1/b/{bucket}/o/"
                   f"{urllib.parse.quote(name, safe='')}?alt=media", headers=auth)
        text = raw.decode("utf-16") if raw[:2] in (b"\xff\xfe", b"\xfe\xff") else raw.decode("utf-8-sig")
        for row in csv.DictReader(io.StringIO(text)):
            day, n = row.get("Date"), row.get("Daily User Installs")
            if day and n not in (None, ""):
                per_day[day] = int(n)
    return (per_day or None), (None if per_day else "no installs overview CSVs in the bucket")


# ---------------------------------------------------------------- main

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dry-run", action="store_true", help="print the payload, send nothing")
    args = ap.parse_args()

    today = dt.datetime.now(dt.timezone.utc).date().isoformat()
    payload = {"daily": [], "totals": []}
    summary, failures = {}, []

    try:
        total, detail = collect_github()
        payload["totals"].append({"channel": "github", "day": today, "total": total, "detail": detail})
        summary["github"] = total
    except Exception as e:
        failures.append(f"github: {e}")

    try:
        n = collect_chocolatey()
        payload["totals"].append({"channel": "chocolatey", "day": today, "total": n})
        summary["chocolatey (not in total)"] = n
    except Exception as e:
        failures.append(f"chocolatey: {e}")

    for ch, fn in (("appstore", collect_appstore), ("msstore", collect_msstore), ("play", collect_play)):
        try:
            per_day, why = fn()
        except Exception as e:
            failures.append(f"{ch}: {e}")
            continue
        if per_day is None:
            summary[ch] = f"skipped ({why})"
            continue
        for day, n in sorted(per_day.items()):
            payload["daily"].append({"channel": ch, "day": day, "count": n})
        summary[ch] = sum(per_day.values())

    for k, v in summary.items():
        log(f"  {k:28} {v}")
    for f in failures:
        log(f"  FAILED {f}")

    if args.dry_run:
        print(json.dumps(payload, ensure_ascii=False)[:4000])
    else:
        token = os.environ.get("SOLOMD_INGEST_TOKEN")
        if not token:
            sys.exit("SOLOMD_INGEST_TOKEN is not set")
        url = os.environ.get("SOLOMD_INGEST_URL", "https://solomd.app/api/admin/downloads")
        res = http(url, data=json.dumps(payload).encode(), method="POST",
                   headers={"Content-Type": "application/json", "Authorization": f"Bearer {token}"})
        log(f"  sent: {res.decode()}")

    # A failed channel fails the run (so the workflow shows red), but only
    # after everything that did work has been sent.
    if failures:
        sys.exit(1)


if __name__ == "__main__":
    main()
