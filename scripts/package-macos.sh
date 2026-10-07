#!/usr/bin/env bash
# Builds a universal (Apple Silicon + Intel) arcmin.app and a .dmg in ./dist.
# Run on macOS:  scripts/package-macos.sh
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
APP=dist/arcmin.app
DMG="dist/arcmin-${VERSION}-macos-universal.dmg"

rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null
cargo build --release --target aarch64-apple-darwin
cargo build --release --target x86_64-apple-darwin

rm -rf dist
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
lipo -create \
  target/aarch64-apple-darwin/release/arcmin \
  target/x86_64-apple-darwin/release/arcmin \
  -output "$APP/Contents/MacOS/arcmin"
cp assets/icon.icns "$APP/Contents/Resources/icon.icns"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>arcmin</string>
  <key>CFBundleDisplayName</key><string>arcmin</string>
  <key>CFBundleIdentifier</key><string>io.github.emincmg.arcmin</string>
  <key>CFBundleExecutable</key><string>arcmin</string>
  <key>CFBundleIconFile</key><string>icon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${VERSION}</string>
  <key>CFBundleVersion</key><string>${VERSION}</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.productivity</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSPrincipalClass</key><string>NSApplication</string>
</dict>
</plist>
PLIST

# Ad-hoc signature: required for arm64 binaries to launch. It is not a Developer ID
# signature, so Gatekeeper still asks the user to confirm the first launch.
codesign --force --deep --sign - "$APP"
codesign --verify --deep --strict "$APP"

hdiutil create -volname "arcmin ${VERSION}" -srcfolder "$APP" -ov -format UDZO "$DMG" >/dev/null
echo "Built $APP and $DMG"
lipo -info "$APP/Contents/MacOS/arcmin"
