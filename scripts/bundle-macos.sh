#!/bin/bash
# Builds dist/Whisper.app: the release binary, Info.plist, icons, the Swift
# helpers and MediaRemoteAdapter from resources/bin, and ffmpeg when present
# (see scripts/download-ffmpeg.sh). Signs ad hoc (identity "-"), or with
# --release, with the persistent release identity (see docs/macos-signing.md).
#
# Usage: scripts/bundle-macos.sh [--release] [--skip-build]

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

release=0
skip_build=0
for arg in "$@"; do
  case "$arg" in
    --release) release=1 ;;
    --skip-build) skip_build=1 ;;
    *)
      echo "Usage: $0 [--release] [--skip-build]" >&2
      exit 1
      ;;
  esac
done

if [[ "$skip_build" == 0 ]]; then
  cargo build --release --locked
fi

version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
app="$root/dist/Whisper.app"
contents="$app/Contents"

rm -rf "$app"
mkdir -p "$contents/MacOS" "$contents/Resources/bin"

cp target/release/whisper "$contents/MacOS/whisper"
sed "s/__VERSION__/$version/g" packaging/Info.plist > "$contents/Info.plist"
cp assets/icon.icns "$contents/Resources/icon.icns"
cp assets/Assets.car "$contents/Resources/Assets.car"
cp LICENSE "$contents/Resources/"

bin="$contents/Resources/bin"
for helper in macos-globe-listener macos-fast-paste macos-text-monitor macos-media-remote macos-mic-listener; do
  if [[ ! -x "resources/bin/$helper" ]]; then
    echo "[bundle] missing resources/bin/$helper (cargo build compiles the Swift helpers)" >&2
    exit 1
  fi
  cp "resources/bin/$helper" "$bin/"
done
if [[ -d resources/bin/MediaRemoteAdapter.framework ]]; then
  cp -R resources/bin/MediaRemoteAdapter.framework "$bin/"
  cp resources/mediaremote-adapter/bin/mediaremote-adapter.pl "$bin/"
  cp resources/mediaremote-adapter/LICENSE "$bin/mediaremote-adapter.LICENSE"
else
  echo "[bundle] MediaRemoteAdapter.framework not built; media pause/resume falls back to media keys" >&2
fi
if [[ -x resources/bin/ffmpeg ]]; then
  cp resources/bin/ffmpeg "$bin/"
  if [[ -f resources/bin/ffmpeg.LICENSE ]]; then
    cp resources/bin/ffmpeg.LICENSE "$bin/"
  fi
else
  echo "[bundle] resources/bin/ffmpeg not found; uploads of non-WAV audio need ffmpeg on PATH" >&2
fi

if [[ "$release" == 1 ]]; then
  scripts/sign-macos-release.sh sign "$app"
  echo "[bundle] $app ($version, release signed)"
  exit 0
fi

# Sign nested code first, then the app with the hardened runtime.
find "$bin" -maxdepth 1 -type f -perm -u+x -exec codesign --force --sign - --options runtime {} \;
if [[ -d "$bin/MediaRemoteAdapter.framework" ]]; then
  codesign --force --sign - "$bin/MediaRemoteAdapter.framework"
fi
codesign --force --sign - --options runtime --entitlements packaging/entitlements.plist "$app"
codesign --verify --strict "$app"

echo "[bundle] $app ($version)"
