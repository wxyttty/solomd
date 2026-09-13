#!/usr/bin/env bash
#
# Work out how to authenticate against App Store Connect, once, for every
# script that needs it. Source it; do not execute it.
#
# Prefers an App Store Connect API key over the Apple ID + app-specific
# password pair. The key is not just tidier: the Apple ID path is tied to a
# human's account and its 2FA, which is what forced release steps through a
# browser login that then had to be babysat. A key belongs to the team, does
# not expire on a password change, and works unattended.
#
# API key (preferred) — from App Store Connect → Users and Access → Integrations:
#   ASC_KEY_ID      the key's ID, e.g. ABCD1234EF
#   ASC_ISSUER_ID   the issuer UUID, shown above the key list
#   ASC_KEY_PATH    path to the downloaded AuthKey_<ASC_KEY_ID>.p8
#                   (optional if the file already sits in a standard location)
#
# Fallback:
#   APPLE_ID, APPLE_PASSWORD, APPLE_TEAM_ID
#
# After sourcing, these are set in the caller's shell:
#   ASC_AUTH_MODE    "apikey" or "appleid"
#   ASC_ALTOOL_AUTH  array of flags for `xcrun altool`
#   ASC_NOTARY_AUTH  array of flags for `xcrun notarytool`

# Apple downloads the key once and never again, so the file usually lands in
# ~/Downloads and stays there. altool will not take a path — it only looks in
# these four directories, by filename. notarytool, in contrast, takes a path.
ASC_KEY_SEARCH_DIRS=(
  "./private_keys"
  "$HOME/private_keys"
  "$HOME/.private_keys"
  "$HOME/.appstoreconnect/private_keys"
)

asc_locate_key() {
  local key_id="$1"
  if [ -n "${ASC_KEY_PATH:-}" ] && [ -f "$ASC_KEY_PATH" ]; then
    printf '%s\n' "$ASC_KEY_PATH"
    return 0
  fi
  local d
  for d in "${ASC_KEY_SEARCH_DIRS[@]}"; do
    if [ -f "$d/AuthKey_${key_id}.p8" ]; then
      printf '%s\n' "$d/AuthKey_${key_id}.p8"
      return 0
    fi
  done
  return 1
}

# altool finds the key by name in one of its four directories and offers no
# way to point at a file. Rather than copying a private key around, link it —
# the secret stays in exactly one place on disk.
asc_link_key_for_altool() {
  local key_id="$1" src="$2" dest_dir="$HOME/.appstoreconnect/private_keys"
  local dest="$dest_dir/AuthKey_${key_id}.p8"
  local d
  for d in "${ASC_KEY_SEARCH_DIRS[@]}"; do
    [ -f "$d/AuthKey_${key_id}.p8" ] && return 0
  done
  mkdir -p "$dest_dir"
  ln -sf "$(cd "$(dirname "$src")" && pwd)/$(basename "$src")" "$dest"
  echo "    linked $src -> $dest (altool only reads keys by name from its own directories)" >&2
}

asc_resolve_auth() {
  if [ -n "${ASC_KEY_ID:-}" ] && [ -n "${ASC_ISSUER_ID:-}" ]; then
    local key
    if ! key="$(asc_locate_key "$ASC_KEY_ID")"; then
      echo "ERROR: ASC_KEY_ID/ASC_ISSUER_ID are set but AuthKey_${ASC_KEY_ID}.p8 was not found." >&2
      echo "       Set ASC_KEY_PATH to it, or drop it in ~/.appstoreconnect/private_keys/." >&2
      exit 1
    fi
    asc_link_key_for_altool "$ASC_KEY_ID" "$key"
    ASC_AUTH_MODE="apikey"
    # No --asc-provider: with a key the team is implied, and passing it errors.
    ASC_ALTOOL_AUTH=(--apiKey "$ASC_KEY_ID" --apiIssuer "$ASC_ISSUER_ID")
    ASC_NOTARY_AUTH=(--key "$key" --key-id "$ASC_KEY_ID" --issuer "$ASC_ISSUER_ID")
    echo "==> App Store Connect auth: API key $ASC_KEY_ID"
    return 0
  fi

  if [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_PASSWORD:-}" ]; then
    : "${APPLE_TEAM_ID:?Set APPLE_TEAM_ID (needed with Apple ID auth)}"
    ASC_AUTH_MODE="appleid"
    ASC_ALTOOL_AUTH=(-u "$APPLE_ID" -p "$APPLE_PASSWORD" --asc-provider "$APPLE_TEAM_ID")
    ASC_NOTARY_AUTH=(--apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" --team-id "$APPLE_TEAM_ID")
    echo "==> App Store Connect auth: Apple ID $APPLE_ID"
    echo "    (an API key would avoid this account's 2FA — see scripts/lib/asc-auth.sh)"
    return 0
  fi

  echo "ERROR: no App Store Connect credentials." >&2
  echo "       Set ASC_KEY_ID + ASC_ISSUER_ID (+ ASC_KEY_PATH), or APPLE_ID + APPLE_PASSWORD." >&2
  exit 1
}
