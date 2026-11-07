#!/bin/bash
# Rebuild compiled macOS icons and README previews from the Icon Composer source.
# Requires full Xcode with Icon Composer (Xcode 26 or later).

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
developer="$(xcode-select -p)"
ictool="$developer/../Applications/Icon Composer.app/Contents/Executables/ictool"
if [[ ! -x "$ictool" ]]; then
  echo "Icon Composer is missing from the selected Xcode installation." >&2
  exit 1
fi

work="$(mktemp -d "${TMPDIR:-/tmp}/whisper-icons.XXXXXX")"
trap 'rm -rf "$work"' EXIT

xcrun actool assets/whisper.icon \
  --compile "$work" --app-icon whisper --include-all-app-icons \
  --target-device mac --minimum-deployment-target 12.0 --platform macosx \
  --output-partial-info-plist "$work/icon.plist" \
  --output-format human-readable-text --warnings --errors

for rendition in Default Dark; do
  "$ictool" assets/whisper.icon --export-image \
    --output-file "$work/$rendition.png" --platform macOS \
    --rendition "$rendition" --width 1024 --height 1024 --scale 1
  sips -Z 256 "$work/$rendition.png" --out "$work/$rendition-preview.png" >/dev/null
done

# actool's compatibility ICNS can omit sizes above 256 px. Export the complete
# classic icon family from the default rendition for older macOS versions.
mkdir "$work/whisper.iconset"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$work/Default.png" \
    --out "$work/whisper.iconset/icon_${size}x${size}.png" >/dev/null
  doubled=$((size * 2))
  sips -z "$doubled" "$doubled" "$work/Default.png" \
    --out "$work/whisper.iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$work/whisper.iconset" -o "$work/icon.icns"

cp "$work/Assets.car" assets/Assets.car
cp "$work/icon.icns" assets/icon.icns
cp "$work/Default-preview.png" docs/images/icon.png
cp "$work/Dark-preview.png" docs/images/icon-dark.png

echo "Updated macOS app icons and light/dark README previews."
