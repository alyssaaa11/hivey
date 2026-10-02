#!/bin/bash
# Builds Hiver.app (universal: Apple Silicon + Intel) and dist/Hiver.zip for sharing
set -euo pipefail
cd "$(dirname "$0")"

APP=Hiver.app
rm -rf "$APP" dist
mkdir -p "$APP/Contents/MacOS" dist

for arch in arm64 x86_64; do
    swiftc -O -target "$arch-apple-macos12" main.swift ../shared/HiverWatch.swift -o "/tmp/hiver-$arch"
done
lipo -create /tmp/hiver-arm64 /tmp/hiver-x86_64 -output "$APP/Contents/MacOS/hiver"
rm -f /tmp/hiver-arm64 /tmp/hiver-x86_64

cat > "$APP/Contents/Info.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key><string>com.hiver.pet</string>
    <key>CFBundleName</key><string>Hiver</string>
    <key>CFBundleExecutable</key><string>hiver</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>1.0</string>
    <key>LSMinimumSystemVersion</key><string>12.0</string>
    <key>LSUIElement</key><true/>
</dict>
</plist>
EOF

codesign --force --deep -s - "$APP"
ditto -c -k --keepParent "$APP" dist/Hiver.zip
echo "Built $APP and dist/Hiver.zip"
