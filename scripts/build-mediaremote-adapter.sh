#!/bin/bash
# Builds resources/bin/MediaRemoteAdapter.framework from the vendored
# ungive/mediaremote-adapter sources under resources/mediaremote-adapter/.
#
# The framework is loaded at runtime by /usr/bin/perl, the only path that can
# talk to MediaRemote on macOS 15.4+ (see src/native/media.rs). Invoked by
# build.rs.
#
# Usage: scripts/build-mediaremote-adapter.sh [--arch arm64|x64]

set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  exit 0
fi

arch="${TARGET_ARCH:-}"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --arch)
      arch="${2:-}"
      shift 2
      ;;
    *)
      shift
      ;;
  esac
done
if [[ -z "$arch" ]]; then
  case "$(uname -m)" in
    arm64) arch="arm64" ;;
    *) arch="x64" ;;
  esac
fi

case "$arch" in
  arm64)
    clang_target="arm64-apple-macos11.0"
    cpu_hex="0c000001" # CPU_TYPE_ARM64, little-endian bytes 4..7
    ;;
  x64)
    clang_target="x86_64-apple-macos10.15"
    cpu_hex="07000001" # CPU_TYPE_X86_64
    ;;
  *)
    echo "[mra] Unsupported architecture: $arch" >&2
    exit 1
    ;;
esac

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source_root="$project_root/resources/mediaremote-adapter"
output_dir="$project_root/resources/bin"
framework_root="$output_dir/MediaRemoteAdapter.framework"
hash_file="$output_dir/.MediaRemoteAdapter.$arch.hash"

framework_name="MediaRemoteAdapter"
framework_version="A"
bundle_identifier="com.softmaxe.whisper.MediaRemoteAdapter"
short_version="0.7.6"

sources=(
  "src/adapter/env.m"
  "src/adapter/get.m"
  "src/adapter/globals.m"
  "src/adapter/keys.m"
  "src/adapter/now_playing.m"
  "src/adapter/repeat.m"
  "src/adapter/seek.m"
  "src/adapter/send.m"
  "src/adapter/shuffle.m"
  "src/adapter/speed.m"
  "src/adapter/stream.m"
  "src/private/MediaRemote.m"
  "src/utility/Debounce.m"
  "src/utility/helpers.m"
)
public_header="include/MediaRemoteAdapter.h"

log() {
  echo "[mra] $*"
}

# Hash every source, the public header and all private headers, plus the arch.
hash_sources() {
  (
    cd "$source_root"
    {
      echo "$arch"
      for rel in $(printf '%s\n' "${sources[@]}" "$public_header" | sort); do
        echo "$rel"
        cat "$rel"
      done
      for header in $(find src -name '*.h' | sort); do
        echo "$header"
        cat "$header"
      done
    } | shasum -a 256 | awk '{print $1}'
  )
}

verify_arch() {
  local file="$1"
  [[ -f "$file" ]] || return 1
  local header
  header="$(xxd -p -l 8 "$file" 2>/dev/null || true)"
  # MH_MAGIC_64 (0xfeedfacf) little-endian followed by the CPU type.
  [[ "$header" == "cffaedfe$cpu_hex" ]]
}

dylib_path="$framework_root/Versions/$framework_version/$framework_name"
current_hash="$(hash_sources)"

if verify_arch "$dylib_path" && [[ -f "$hash_file" ]] &&
  [[ "$(tr -d '[:space:]' <"$hash_file")" == "$current_hash" ]]; then
  exit 0
fi

mkdir -p "$output_dir"
rm -rf "$framework_root"
version_root="$framework_root/Versions/$framework_version"
mkdir -p "$version_root/Headers" "$version_root/Resources"

ln -sfn "$framework_version" "$framework_root/Versions/Current"
ln -sfn "Versions/Current/$framework_name" "$framework_root/$framework_name"
ln -sfn "Versions/Current/Headers" "$framework_root/Headers"
ln -sfn "Versions/Current/Resources" "$framework_root/Resources"

cp "$source_root/$public_header" "$version_root/Headers/$framework_name.h"

cat >"$version_root/Resources/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDevelopmentRegion</key>
	<string>en</string>
	<key>CFBundleExecutable</key>
	<string>$framework_name</string>
	<key>CFBundleIdentifier</key>
	<string>$bundle_identifier</string>
	<key>CFBundleInfoDictionaryVersion</key>
	<string>6.0</string>
	<key>CFBundleName</key>
	<string>$framework_name</string>
	<key>CFBundlePackageType</key>
	<string>FMWK</string>
	<key>CFBundleShortVersionString</key>
	<string>$short_version</string>
	<key>CFBundleVersion</key>
	<string>$short_version</string>
	<key>NSPrincipalClass</key>
	<string></string>
</dict>
</plist>
PLIST

source_args=()
for rel in "${sources[@]}"; do
  source_args+=("$source_root/$rel")
done

clang_args=(
  -fobjc-arc
  -fvisibility=default
  -O2
  -target "$clang_target"
  -I "$source_root/include"
  -I "$source_root/src"
  "${source_args[@]}"
  -dynamiclib
  "-Wl,-install_name,@rpath/MediaRemoteAdapter.framework/Versions/A/MediaRemoteAdapter"
  -compatibility_version 1.0.0
  -current_version "$short_version"
  -framework Foundation
  -framework AppKit
  -framework ImageIO
  -framework CoreServices
  -framework UniformTypeIdentifiers
  -o "$version_root/$framework_name"
)

log "Building $framework_name.framework for $arch"
if ! xcrun clang "${clang_args[@]}"; then
  if ! clang "${clang_args[@]}"; then
    echo "[mra] Failed to compile MediaRemoteAdapter framework." >&2
    exit 1
  fi
fi

if ! verify_arch "$version_root/$framework_name"; then
  echo "[mra] FATAL: Compiled dylib architecture does not match target ($arch)." >&2
  exit 1
fi

# Ad-hoc codesign so /usr/bin/perl can dlopen the framework. Packaging re-signs
# it with the release identity.
if ! codesign --force --sign - --timestamp=none "$framework_root"; then
  echo "[mra] codesign --force --sign - failed" >&2
  exit 1
fi

printf '%s' "$current_hash" >"$hash_file"
log "Built MediaRemoteAdapter.framework ($arch)"
