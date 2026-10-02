#!/bin/bash
# Builds "Hiver H.app" (universal: Apple Silicon + Intel) and dist/HiverH.zip for sharing
set -euo pipefail
cd "$(dirname "$0")"

APP="Hiver H.app"
rm -rf "$APP" dist
mkdir -p "$APP/Contents/MacOS" dist

for arch in arm64 x86_64; do
    swiftc -O -target "$arch-apple-macos12" main.swift ../shared/HiverWatch.swift -o "/tmp/hiver-h-$arch"
done
lipo -create /tmp/hiver-h-arm64 /tmp/hiver-h-x86_64 -output "$APP/Contents/MacOS/hiver-h"
rm -f /tmp/hiver-h-arm64 /tmp/hiver-h-x86_64

cat > "$APP/Contents/Info.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key><string>com.hiver.h</string>
    <key>CFBundleName</key><string>Hiver H</string>
    <key>CFBundleExecutable</key><string>hiver-h</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>1.0</string>
    <key>LSMinimumSystemVersion</key><string>12.0</string>
    <key>LSUIElement</key><true/>
</dict>
</plist>
EOF

codesign --force --deep -s - "$APP"
ditto -c -k --keepParent "$APP" dist/HiverH.zip
echo "Built $APP and dist/HiverH.zip"
