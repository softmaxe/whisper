#!/bin/bash
# Downloads the pinned static ffmpeg build (ffmpeg-static b6.1.1, arm64) and
# its license into resources/bin, checking both against known SHA-256 digests.
# Release builds bundle this binary; scripts/fetch-ffmpeg.sh copies a local
# ffmpeg instead.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
base="https://github.com/eugeneware/ffmpeg-static/releases/download/b6.1.1"
binary_sha256="a90e3db6a3fd35f6074b013f948b1aa45b31c6375489d39e572bea3f18336584"
license_sha256="cb48bf09a11f5fb576cddb0431c8f5ed0a60157a9ec942adffc13907cbe083f2"

work="$(mktemp -d "${TMPDIR:-/tmp}/whisper-ffmpeg.XXXXXX")"
trap 'rm -rf "$work"' EXIT

curl -fsSL --retry 3 -o "$work/ffmpeg.gz" "$base/ffmpeg-darwin-arm64.gz"
curl -fsSL --retry 3 -o "$work/ffmpeg.LICENSE" "$base/darwin-arm64.LICENSE"
gunzip "$work/ffmpeg.gz"
echo "$binary_sha256  $work/ffmpeg" | shasum -a 256 -c - >/dev/null
echo "$license_sha256  $work/ffmpeg.LICENSE" | shasum -a 256 -c - >/dev/null

mkdir -p "$root/resources/bin"
install -m 755 "$work/ffmpeg" "$root/resources/bin/ffmpeg"
install -m 644 "$work/ffmpeg.LICENSE" "$root/resources/bin/ffmpeg.LICENSE"
echo "[ffmpeg] Installed ffmpeg-static b6.1.1 into resources/bin"
