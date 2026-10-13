#!/usr/bin/env bash
#
# Build SoloMDQuickLook.appex: the Finder Quick Look preview for Markdown (#351).
#
#   app/src-tauri/quicklook/build.sh <version> <out-dir> [signing-identity] [entitlements]
#
# Produces <out-dir>/SoloMDQuickLook.appex (universal arm64 + x86_64, macOS 12+).
# With a signing identity it is signed (hardened runtime, timestamp, sandbox
# entitlements by default). The caller copies it into
# SoloMD.app/Contents/PlugIns/ BEFORE signing the app itself, so the app's
# signature seals it; see scripts/build-mac.sh and scripts/build-mas.sh.
#
# Plain swiftc, no Xcode project: an app extension is an ordinary bundle whose
# executable enters through NSExtensionMain.

set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
APP_DIR="$(cd "$HERE/../.." && pwd)"   # app/
VER="${1:?usage: $0 <version> <out-dir> [signing-identity] [entitlements]}"
OUT="${2:?usage: $0 <version> <out-dir> [signing-identity] [entitlements]}"
IDENTITY="${3:-}"
ENTITLEMENTS="${4:-$HERE/QuickLook.entitlements}"

mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
APPEX="$OUT/SoloMDQuickLook.appex"
rm -rf "$APPEX"
mkdir -p "$APPEX/Contents/MacOS" "$APPEX/Contents/Resources"

echo "==> Quick Look: bundling the Markdown renderer"
ESBUILD="$(find "$APP_DIR/node_modules/.pnpm" -path '*/node_modules/esbuild/bin/esbuild' 2>/dev/null | sort | tail -1)"
[ -x "$ESBUILD" ] || { echo "esbuild not found under app/node_modules (run pnpm install in app/)" >&2; exit 1; }
"$ESBUILD" "$HERE/js/render.mjs" --bundle --format=iife --platform=neutral \
  --main-fields=module,main --target=safari15 --minify --log-level=warning \
  --outfile="$APPEX/Contents/Resources/render.js"
cp "$HERE/Resources/preview.css" "$APPEX/Contents/Resources/"

echo "==> Quick Look: compiling (arm64 + x86_64)"
SDK="$(xcrun --sdk macosx --show-sdk-path)"
for arch in arm64 x86_64; do
  xcrun swiftc -O -sdk "$SDK" -target "$arch-apple-macos12.0" \
    -module-name SoloMDQuickLook -application-extension \
    -framework Cocoa -framework Quartz -framework JavaScriptCore \
    -Xlinker -e -Xlinker _NSExtensionMain \
    "$HERE/Sources/PreviewViewController.swift" \
    -o "$WORK/SoloMDQuickLook-$arch"
done
lipo -create "$WORK/SoloMDQuickLook-arm64" "$WORK/SoloMDQuickLook-x86_64" \
  -output "$APPEX/Contents/MacOS/SoloMDQuickLook"

sed "s/__VERSION__/$VER/g" "$HERE/Info.plist" > "$APPEX/Contents/Info.plist"
plutil -lint "$APPEX/Contents/Info.plist" >/dev/null

if [ -n "$IDENTITY" ]; then
  echo "==> Quick Look: signing"
  codesign --force --options runtime --timestamp \
    --entitlements "$ENTITLEMENTS" --sign "$IDENTITY" "$APPEX"
  codesign --verify --strict "$APPEX"
fi
echo "==> $APPEX"
