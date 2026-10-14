#!/bin/bash
# Validates the runner, toolchain and package version shared by the CI, Build
# and Release workflows. Set RELEASE_TAG to require a matching `v<version>`
# tag. When GITHUB_OUTPUT is set, the version and release archive name are
# written as step outputs.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

fail() {
  echo "[version] $*" >&2
  exit 1
}

[[ "$(uname -m)" == arm64 ]] || fail "Whisper builds require an arm64 runner."

channel="$(sed -n 's/^channel = "\(.*\)"/\1/p' "$root/rust-toolchain.toml")"
[[ -n "$channel" ]] || fail "rust-toolchain.toml must pin a Rust version."
rustc_version="$(cd "$root" && rustc --version)"
[[ "$rustc_version" == "rustc $channel "* ]] || fail "$rustc_version does not match rust-toolchain.toml ($channel)."

version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "Cargo.toml version '$version' is not x.y.z."
locked="$(awk '/^name = "whisper"$/ { getline; gsub(/version = |"/, ""); print; exit }' "$root/Cargo.lock")"
[[ "$locked" == "$version" ]] || fail "Cargo.lock has whisper $locked; Cargo.toml has $version."

if [[ -n "${RELEASE_TAG:-}" && "$RELEASE_TAG" != "v$version" ]]; then
  fail "Tag $RELEASE_TAG does not match version $version."
fi

if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
  printf 'version=%s\narchive=whisper-%s-macos-arm64.zip\n' "$version" "$version" >>"$GITHUB_OUTPUT"
fi
echo "[version] whisper $version ($rustc_version)"
