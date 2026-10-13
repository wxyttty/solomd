#!/usr/bin/env bash
#
# Point the package managers at a published SoloMD release.
#
#   scripts/bump-package-managers.sh <X.Y.Z> [brew|choco|winget|scoop ...]
#
# With no channel names, all four run. Run it after the GitHub release is
# published — every channel downloads from the release's public URLs.
#
# Why this exists: none of these channels was part of the release routine, so
# every one of them rotted on its own. On 2026-09-24 the tap served 3.5.0,
# Chocolatey 0.1.8, Scoop was stuck at 4.11.6 because its autoupdate URL named
# an installer we stopped shipping, and winget never got in at all (every PR
# closed as stale on an unsigned CLA).
#
#   brew    zhitongblog/homebrew-solomd, Casks/solomd.rb — committed directly.
#   choco   community.chocolatey.org package `solomd` — packed and pushed here;
#           each version then waits in Chocolatey's moderation queue.
#   winget  microsoft/winget-pkgs, package zhitongblog.SoloMD — a PR opened by
#           komac (brew install komac); the winget bot validates and merges.
#   scoop   ScoopInstaller/Extras bucket/solomd.json — nothing to do per
#           release: its Excavator bot follows GitHub releases by itself, as
#           long as the autoupdate URL names an asset we still publish. This
#           step only checks that the bucket has caught up and warns if not.
#
# Checksums come from the GitHub API's per-asset `digest`, so nothing large is
# downloaded (the local proxy stalls big downloads — see upload-release-assets.sh).
#
# Requires: gh (authenticated as a maintainer), python3; choco needs
# CHOCOLATEY_API_KEY in .env.local (packed by scripts/lib/pack_nupkg.py, no
# choco or nuget needed); winget needs komac.

set -euo pipefail
cd "$(dirname "$0")/.."

VER="${1:?usage: $0 <X.Y.Z> [brew|choco|winget|scoop ...]}"
shift
CHANNELS=("$@")
[ ${#CHANNELS[@]} -eq 0 ] && CHANNELS=(brew choco winget scoop)

REPO=zhitongblog/solomd
TAG="v$VER"
DL="https://github.com/$REPO/releases/download/$TAG"

if [ -f .env.local ]; then
  set -a
  # shellcheck disable=SC1091
  source .env.local
  set +a
fi

# gh through the proxy answers EOF now and then; never trust a single read.
gh_retry() {
  local out i
  for i in 1 2 3 4 5 6; do
    if out="$(gh "$@" 2>/dev/null)" && [ -n "$out" ]; then
      printf '%s\n' "$out"
      return 0
    fi
    sleep 5
  done
  echo "gh $* failed after retries" >&2
  return 1
}

# sha256 of a published asset, from the API — no download.
sha_of() {
  local d
  d="$(gh_retry api "repos/$REPO/releases/tags/$TAG" \
        --jq ".assets[] | select(.name == \"$1\" and .state == \"uploaded\") | .digest")"
  case "$d" in
    sha256:*) echo "${d#sha256:}" ;;
    *) echo "no finished asset $1 on $TAG" >&2; return 1 ;;
  esac
}

bump_brew() {
  local tap=zhitongblog/homebrew-solomd path=Casks/solomd.rb sha tmp blob
  sha="$(sha_of "SoloMD_${VER}_universal.dmg")"
  tmp="$(mktemp -d)"
  gh_retry api "repos/$tap/contents/$path" > "$tmp/cur.json"
  blob="$(python3 -c 'import sys,json;print(json.load(open(sys.argv[1]))["sha"])' "$tmp/cur.json")"
  # A function replacement, not a template string: an earlier f-string version
  # wrote `version \"4.14.2\"` into the cask and broke it for every user.
  VER="$VER" SHA="$sha" python3 - "$tmp/cur.json" "$tmp/solomd.rb" <<'PY'
import base64, json, os, re, sys
ver, sha = os.environ["VER"], os.environ["SHA"]
src = base64.b64decode(json.load(open(sys.argv[1]))["content"]).decode()
q = chr(34)
src, n1 = re.subn(r'^(\s*version )\\?"[^"\\]+\\?"', lambda m: m.group(1) + q + ver + q, src, count=1, flags=re.M)
src, n2 = re.subn(r'^(\s*sha256 )\\?"[0-9a-f]+\\?"', lambda m: m.group(1) + q + sha + q, src, count=1, flags=re.M)
assert n1 == 1 and n2 == 1, "cask layout changed"
open(sys.argv[2], "w").write(src)
PY
  ruby -c "$tmp/solomd.rb" >/dev/null || { echo "brew     generated cask does not parse" >&2; return 1; }
  if cmp -s "$tmp/solomd.rb" <(python3 -c 'import sys,json,base64;sys.stdout.write(base64.b64decode(json.load(open(sys.argv[1]))["content"]).decode())' "$tmp/cur.json"); then
    echo "brew     already $VER"
    return 0
  fi
  gh_retry api -X PUT "repos/$tap/contents/$path" \
    -f message="solomd $VER" \
    -f content="$(base64 < "$tmp/solomd.rb")" \
    -f sha="$blob" --jq .commit.sha >/dev/null
  echo "brew     $tap -> $VER ($sha)"
}

bump_choco() {
  local src=distribution/manifests/chocolatey sha tmp nupkg
  : "${CHOCOLATEY_API_KEY:?CHOCOLATEY_API_KEY missing (.env.local)}"
  if [ "$(curl -s --max-time 30 "https://community.chocolatey.org/api/v2/Packages(Id='solomd',Version='$VER')" -o /dev/null -w '%{http_code}')" = 200 ]; then
    echo "choco    already has $VER (it may still be in moderation)"
    return 0
  fi
  sha="$(sha_of "SoloMD_${VER}_x64_en-US.msi")"
  tmp="$(mktemp -d)"
  cp -R "$src"/. "$tmp"/
  find "$tmp" -type f \( -name '*.nuspec' -o -name '*.ps1' \) -exec \
    sed -i '' -e "s/{{VERSION}}/$VER/g" -e "s/{{SHA64}}/$sha/g" {} +
  if grep -rq '{{' "$tmp"; then echo "choco    unfilled placeholder" >&2; return 1; fi
  nupkg="$(mktemp -d)/solomd.$VER.nupkg"
  python3 scripts/lib/pack_nupkg.py "$tmp" "$nupkg" >/dev/null
  # NuGet's v2 push: a multipart PUT with the key in a header.
  local code
  code="$(curl -s -o "$tmp/push.out" -w '%{http_code}' --max-time 300 -X PUT \
    -H "X-NuGet-ApiKey: $CHOCOLATEY_API_KEY" -F "package=@$nupkg" \
    https://push.chocolatey.org/api/v2/package/)"
  case "$code" in
    200|201|202) ;;
    *) echo "choco    push failed: HTTP $code $(head -c 300 "$tmp/push.out")" >&2; return 1 ;;
  esac
  echo "choco    pushed $VER ($sha) — now in moderation: https://community.chocolatey.org/packages/solomd/$VER"
}

bump_winget() {
  # komac update needs the package to already exist in winget-pkgs (zhitong.SoloMD,
  # first merged via PR #440523). It downloads both MSIs to read ProductCode etc.
  command -v komac >/dev/null || { echo "winget   komac missing: brew install komac" >&2; return 1; }
  if gh api "repos/microsoft/winget-pkgs/contents/manifests/z/zhitong/SoloMD/$VER" >/dev/null 2>&1; then
    echo "winget   already has $VER"
    return 0
  fi
  # Until the first PR merges the package doesn't exist upstream and komac fails;
  # judge by komac's own output, not by reaching the end of this function.
  local out
  out="$(GITHUB_TOKEN="$(gh auth token)" komac update zhitong.SoloMD --version "$VER" \
    --urls "$DL/SoloMD_${VER}_x64_en-US.msi" "$DL/SoloMD_${VER}_arm64_en-US.msi" \
    --release-notes-url "https://github.com/$REPO/releases/tag/$TAG" \
    --submit < /dev/null 2>&1)" || true
  if grep -q "pull/" <<<"$out"; then
    echo "winget   PR opened for $VER: $(grep -o 'https://github.com/[^ ]*pull/[0-9]*' <<<"$out" | head -1)"
  else
    echo "winget   FAILED for $VER — komac said:" >&2
    tail -5 <<<"$out" >&2
    return 1
  fi
}

check_scoop() {
  # Nothing to push: ScoopInstaller/Extras' Excavator bot reads GitHub releases
  # and SHA256SUMS.txt on its own, usually within a few hours. Just report.
  local v
  v="$(gh_retry api repos/ScoopInstaller/Extras/contents/bucket/solomd.json --jq .content | base64 -d \
       | python3 -c 'import sys,json;print(json.load(sys.stdin)["version"])')"
  if [ "$v" = "$VER" ]; then
    echo "scoop    Extras is on $VER"
  else
    echo "scoop    Extras is on $v; Excavator should pick up $VER within hours. If it stays behind for a day, check its autoupdate still matches the asset names."
  fi
}

# One channel failing (winget, until its first PR merges) must not skip the
# channels after it; the exit status still reports that something failed.
rc=0
for ch in "${CHANNELS[@]}"; do
  case "$ch" in
    brew)   bump_brew || rc=1 ;;
    choco)  bump_choco || rc=1 ;;
    winget) bump_winget || rc=1 ;;
    scoop)  check_scoop || rc=1 ;;
    *) echo "unknown channel: $ch" >&2; exit 1 ;;
  esac
done
exit $rc
