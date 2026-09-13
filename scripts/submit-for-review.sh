#!/usr/bin/env bash
#
# Submit an already-uploaded build for App Store review.
#
# `submit-ios.sh` / `submit-mas.sh` upload a binary; this is the step after —
# create the version, attach the build, write the release notes, hand it to
# review. It replaces the browser login that used to end every Apple release.
#
# Usage:
#   ./scripts/submit-for-review.sh --platform ios   --version 4.12.0 --notes-file notes.txt
#   ./scripts/submit-for-review.sh --platform macos --version 4.12.0 --dry-run
#
# Requires an App Store Connect API key — there is no Apple ID fallback,
# because this part of the API accepts nothing else:
#   ASC_KEY_ID, ASC_ISSUER_ID, ASC_KEY_PATH   (see scripts/lib/asc-auth.sh)

set -euo pipefail

cd "$(dirname "$0")/.."

if [ -f .env.local ]; then
  set -a
  # shellcheck disable=SC1091
  source .env.local
  set +a
fi

# Reading the usage must not require credentials — someone running --help is
# usually someone who has not set them up yet.
for arg in "$@"; do
  case "$arg" in
    -h|--help) exec python3 scripts/lib/asc_submit.py --help ;;
  esac
done

# shellcheck source=lib/asc-auth.sh
source "$(dirname "$0")/lib/asc-auth.sh"

if [ -z "${ASC_KEY_ID:-}" ] || [ -z "${ASC_ISSUER_ID:-}" ]; then
  echo "ERROR: submitting for review needs an App Store Connect API key." >&2
  echo "       Set ASC_KEY_ID and ASC_ISSUER_ID in .env.local — an Apple ID and" >&2
  echo "       app-specific password cannot reach this part of the API." >&2
  echo "       See the 'Apple credentials' section of scripts/README.md." >&2
  exit 1
fi

# The Python side takes a path; resolve it the same way altool's copy is found
# so a key already sitting in a standard directory needs no extra config.
if [ -z "${ASC_KEY_PATH:-}" ] || [ ! -f "${ASC_KEY_PATH}" ]; then
  if ! ASC_KEY_PATH="$(asc_locate_key "$ASC_KEY_ID")"; then
    echo "ERROR: AuthKey_${ASC_KEY_ID}.p8 not found. Set ASC_KEY_PATH." >&2
    exit 1
  fi
  export ASC_KEY_PATH
fi

exec python3 scripts/lib/asc_submit.py "$@"
