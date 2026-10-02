#!/bin/bash
# Builds "Hiver Prompt.app" (universal: Apple Silicon + Intel) and dist/HiverPrompt.zip for sharing
set -euo pipefail
cd "$(dirname "$0")"

APP="Hiver Prompt.app"
rm -rf "$APP" dist
mkdir -p "$APP/Contents/MacOS" dist

for arch in arm64 x86_64; do
    swiftc -O -target "$arch-apple-macos12" main.swift ../shared/HiverWatch.swift -o "/tmp/hiver-prompt-$arch"
done
lipo -create /tmp/hiver-prompt-arm64 /tmp/hiver-prompt-x86_64 -output "$APP/Contents/MacOS/hiver-prompt"
rm -f /tmp/hiver-prompt-arm64 /tmp/hiver-prompt-x86_64

cat > "$APP/Contents/Info.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key><string>com.hiver.prompt</string>
    <key>CFBundleName</key><string>Hiver Prompt</string>
    <key>CFBundleExecutable</key><string>hiver-prompt</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>1.0</string>
    <key>LSMinimumSystemVersion</key><string>12.0</string>
    <key>LSUIElement</key><true/>
</dict>
</plist>
EOF

codesign --force --deep -s - "$APP"
ditto -c -k --keepParent "$APP" dist/HiverPrompt.zip
echo "Built $APP and dist/HiverPrompt.zip"
