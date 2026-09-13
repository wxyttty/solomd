#!/bin/bash
# make-dmg.sh — 把 solomd GPUI 版打成 macOS ARM 的 .app + .dmg（本地安装用）。
# 用法：cd gpui && scripts/make-dmg.sh
# 产物：target/dmg/Solomd.app 与 target/dmg/Solomd-aarch64.dmg（ad-hoc 签名，
# 首次打开右键 → 打开 绕过 Gatekeeper；runtime_shaders 使首启稍慢）。
set -euo pipefail
cd "$(dirname "$0")/.."

APP_NAME="Solomd"
BIN="target/release/solomd"
OUT="target/dmg"
PLIST="$OUT/$APP_NAME.app/Contents/Info.plist"

[ -f "$BIN" ] || { echo "先构建：cargo build --release -p solomd-app"; exit 1; }

rm -rf "$OUT"
mkdir -p "$OUT/$APP_NAME.app/Contents/MacOS" "$OUT/$APP_NAME.app/Contents/Resources"

cp "$BIN" "$OUT/$APP_NAME.app/Contents/MacOS/solomd"
cp ../brand/file_icon.icns "$OUT/$APP_NAME.app/Contents/Resources/solomd.icns"
cp ../README.md "$OUT/$APP_NAME.app/Contents/Resources/README.md"

cat > "$PLIST" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>$APP_NAME</string>
    <key>CFBundleDisplayName</key><string>SoloMD (GPUI)</string>
    <key>CFBundleIdentifier</key><string>app.solomd.gpui</string>
    <key>CFBundleExecutable</key><string>solomd</string>
    <key>CFBundleIconFile</key><string>solomd</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>0.1.0</string>
    <key>CFBundleVersion</key><string>0.1.0</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>CFBundleDevelopmentRegion</key><string>zh-CN</string>
</dict>
</plist>
EOF

codesign --force --deep -s - "$OUT/$APP_NAME.app"

hdiutil create -volname "$APP_NAME" -srcfolder "$OUT/$APP_NAME.app" \
    -ov -format UDZO "$OUT/$APP_NAME-aarch64.dmg"

echo "产物："
ls -lh "$OUT"
