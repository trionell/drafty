#!/bin/sh
# Build Drafty.app for macOS.
#
#   ./scripts/bundle-macos.sh               app for this Mac's architecture
#   ./scripts/bundle-macos.sh --universal   one app for Apple Silicon and Intel
#
# Then: cp -r target/Drafty.app /Applications/
set -eu
cd "$(dirname "$0")/.."

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
APP=target/Drafty.app

if [ "${1:-}" = "--universal" ]; then
    rustup target add aarch64-apple-darwin x86_64-apple-darwin
    cargo build --release --target aarch64-apple-darwin
    cargo build --release --target x86_64-apple-darwin
    BIN=target/drafty-universal
    lipo -create -output "$BIN" \
        target/aarch64-apple-darwin/release/drafty \
        target/x86_64-apple-darwin/release/drafty
else
    cargo build --release
    BIN=target/release/drafty
fi

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/drafty"

ICONSET=target/drafty.iconset
rm -rf "$ICONSET"
mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
    sips -z $size $size assets/icon-1024.png --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z $double $double assets/icon-1024.png --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/drafty.icns"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>Drafty</string>
    <key>CFBundleDisplayName</key><string>Drafty</string>
    <key>CFBundleIdentifier</key><string>local.drafty.Drafty</string>
    <key>CFBundleExecutable</key><string>drafty</string>
    <key>CFBundleIconFile</key><string>drafty</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$VERSION</string>
    <key>CFBundleVersion</key><string>$VERSION</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

# Apple Silicon refuses to run unsigned binaries. An ad-hoc signature is enough for a
# locally built app; it isn't notarized, so a copy downloaded from elsewhere would still
# need right-click > Open the first time.
codesign --force --sign - "$APP"

echo "Built $APP"
