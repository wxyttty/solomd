#!/usr/bin/env bash
#
# Upload a built iOS .ipa to App Store Connect via altool.
#
# Usage: ./scripts/submit-ios.sh [path/to/SoloMD.ipa]
#
# Defaults to app/src-tauri/gen/apple/build/arm64/SoloMD.ipa when no arg.
#
# Credentials (from .env.local) — an App Store Connect API key is preferred
# and used automatically when present; see scripts/lib/asc-auth.sh:
#   ASC_KEY_ID + ASC_ISSUER_ID (+ ASC_KEY_PATH)     preferred
#   APPLE_ID + APPLE_PASSWORD + APPLE_TEAM_ID       fallback

set -euo pipefail

cd "$(dirname "$0")/.."

if [ -f .env.local ]; then
  set -a
  # shellcheck disable=SC1091
  source .env.local
  set +a
fi

# shellcheck source=lib/asc-auth.sh
source "$(dirname "$0")/lib/asc-auth.sh"
asc_resolve_auth

IPA="${1:-app/src-tauri/gen/apple/build/arm64/SoloMD.ipa}"
[ -f "$IPA" ] || { echo "ERROR: $IPA not found. Run ./scripts/build-ios.sh first." >&2; exit 1; }

echo "==> Validating $IPA against App Store Connect"
xcrun altool --validate-app \
  -f "$IPA" \
  -t ios \
  "${ASC_ALTOOL_AUTH[@]}"

echo ""
echo "==> Uploading $IPA"
xcrun altool --upload-app \
  -f "$IPA" \
  -t ios \
  "${ASC_ALTOOL_AUTH[@]}"

echo ""
echo "==> Upload complete. Build will appear in App Store Connect after ~5-15 min."
echo "    Builds:   https://appstoreconnect.apple.com/apps/6762498874/testflight/ios"
echo "    Versions: https://appstoreconnect.apple.com/apps/6762498874/distribution/ios"
