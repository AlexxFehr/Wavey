#!/usr/bin/env bash
# Builds Wavey.app (release) without Xcode: compiles the Rust core and the
# SwiftPM app, then assembles and ad-hoc signs the bundle.
set -euo pipefail
export PATH="$HOME/.cargo/bin:$PATH"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/target.nosync"
SCRATCH="$OUT/swift"
APP="$OUT/Wavey.app"
VERSION="$(grep -m1 '^version' "$ROOT/Cargo.toml" | sed 's/.*"\(.*\)"/\1/')"

"$ROOT/scripts/build-core.sh" release
(cd "$ROOT/app" && swift build -c release --scratch-path "$SCRATCH")

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$SCRATCH/release/Wavey" "$APP/Contents/MacOS/Wavey"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Wavey</string>
  <key>CFBundleDisplayName</key><string>Wavey</string>
  <key>CFBundleIdentifier</key><string>app.wavey.Wavey</string>
  <key>CFBundleExecutable</key><string>Wavey</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${VERSION}</string>
  <key>CFBundleVersion</key><string>${VERSION}</string>
  <key>LSMinimumSystemVersion</key><string>15.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>CFBundleDocumentTypes</key>
  <array>
    <dict>
      <key>CFBundleTypeName</key><string>Wavey Project</string>
      <key>CFBundleTypeRole</key><string>Editor</string>
      <key>LSTypeIsPackage</key><true/>
      <key>LSItemContentTypes</key><array><string>app.wavey.project</string></array>
    </dict>
  </array>
  <key>UTExportedTypeDeclarations</key>
  <array>
    <dict>
      <key>UTTypeIdentifier</key><string>app.wavey.project</string>
      <key>UTTypeDescription</key><string>Wavey Project</string>
      <key>UTTypeConformsTo</key><array><string>com.apple.package</string></array>
      <key>UTTypeTagSpecification</key>
      <dict><key>public.filename-extension</key><array><string>wavey</string></array></dict>
    </dict>
  </array>
</dict>
</plist>
PLIST
codesign --force --sign - "$APP" >/dev/null
echo "built $APP"
