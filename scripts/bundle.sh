#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
profile="${1:-release}"
bash scripts/fetch-sparkle.sh
if [[ "$profile" == release ]]; then cargo build --release --locked; else cargo build --locked; fi
app="dist/Petalook.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" "$app/Contents/Frameworks" target/AppIcon.iconset
cp "target/$profile/petalook" "$app/Contents/MacOS/Petalook"
cp assets/app-icon.png assets/sunflower.png "$app/Contents/Resources/"
for size in 16 32 128 256 512; do
    sips -z "$size" "$size" assets/app-icon.png --out "target/AppIcon.iconset/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z "$double" "$double" assets/app-icon.png --out "target/AppIcon.iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns target/AppIcon.iconset -o "$app/Contents/Resources/AppIcon.icns"
python3 scripts/prepare-bundle.py "$app/Contents/Info.plist"
ditto target/sparkle/2.10.0/Sparkle.framework "$app/Contents/Frameworks/Sparkle.framework"
cp target/sparkle/2.10.0/LICENSE "$app/Contents/Resources/Sparkle-LICENSE.txt"
identity="${PETALOOK_SIGNING_IDENTITY:--}"
sign_options=(--force --sign "$identity")
if [[ "$identity" != - ]]; then sign_options+=(--options runtime --timestamp); fi
framework="$app/Contents/Frameworks/Sparkle.framework"
for helper in "$framework"/Versions/B/XPCServices/*.xpc "$framework/Versions/B/Autoupdate" "$framework/Versions/B/Updater.app"; do
    codesign "${sign_options[@]}" "$helper"
done
codesign "${sign_options[@]}" "$framework"
codesign "${sign_options[@]}" "$app"
codesign --verify --deep --strict "$app"
echo "Built $PWD/$app"
