#!/bin/bash
# Copies the system ffmpeg (the one on PATH) into resources/bin/ffmpeg so it
# can be bundled into the app. Not run by build.rs; run it before packaging.
#
# Usage: scripts/fetch-ffmpeg.sh [path-to-ffmpeg]
#
# Note: a Homebrew ffmpeg links against Homebrew dylibs and will not run on a
# machine without them. Prefer a static build for release packaging; this
# script warns when the binary has non-system dynamic dependencies.

set -euo pipefail

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output_dir="$project_root/resources/bin"
target="$output_dir/ffmpeg"

source_bin="${1:-$(command -v ffmpeg || true)}"
if [[ -z "$source_bin" ]]; then
  echo "[ffmpeg] ffmpeg not found on PATH. Install it (brew install ffmpeg) or pass a path." >&2
  exit 1
fi

# Follow symlinks (Homebrew installs a link into bin/).
resolved="$(cd "$(dirname "$source_bin")" && pwd -P)/$(basename "$source_bin")"
while [[ -L "$resolved" ]]; do
  link="$(readlink "$resolved")"
  case "$link" in
    /*) resolved="$link" ;;
    *) resolved="$(cd "$(dirname "$resolved")" && cd "$(dirname "$link")" && pwd -P)/$(basename "$link")" ;;
  esac
done

mkdir -p "$output_dir"
cp -f "$resolved" "$target"
chmod 755 "$target"
echo "[ffmpeg] Copied $resolved -> $target"

if command -v otool >/dev/null 2>&1; then
  external="$(otool -L "$target" | tail -n +2 | awk '{print $1}' |
    grep -v -E '^(/usr/lib/|/System/Library/)' || true)"
  if [[ -n "$external" ]]; then
    echo "[ffmpeg] Warning: this ffmpeg links non-system libraries and is not portable:" >&2
    echo "$external" | sed 's/^/  /' >&2
  fi
fi
