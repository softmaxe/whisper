#!/bin/bash
# Signs an app bundle with the persistent release identity, or verifies a
# signed bundle against the pinned certificate. See docs/macos-signing.md.
#
# Usage: scripts/sign-macos-release.sh sign|verify /path/to/Whisper.app
#
# `sign` reads WHISPER_SIGNING_CERTIFICATE (base64 PKCS#12) and
# WHISPER_SIGNING_PASSWORD, or, when both are unset, the saved identity in
# ~/.config/whisper/signing/. It imports the identity into a temporary
# keychain that is removed on exit, success or failure.
#
# Every signed component gets a designated requirement bound to its identifier
# and the pinned certificate, so macOS keeps one identity across releases.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
certificate_pem="$root/packaging/signing-certificate.pem"
entitlements="$root/packaging/entitlements.plist"
app_id="com.softmaxe.whisper"
signing_dir="$HOME/.config/whisper/signing"

die() {
  echo "[sign] $*" >&2
  exit 1
}

fingerprint() {
  local value
  value="$(openssl x509 -in "$certificate_pem" -noout -fingerprint -sha1 2>/dev/null)" ||
    die "The pinned signing certificate is missing or invalid."
  value="${value##*=}"
  value="${value//:/}"
  [[ "$value" =~ ^[0-9A-Fa-f]{40}$ ]] || die "Invalid signing certificate fingerprint."
  printf '%s' "$value" | tr 'a-f' 'A-F'
}

requirement() {
  printf 'identifier "%s" and certificate leaf = H"%s"' "$1" "$2"
}

# Lowercases certificate hashes so requirements compare as text.
normalize() {
  perl -pe 's/H"([0-9A-Fa-f]{40})"/H"\L$1"/g'
}

# Nested code first, then the app; one "identifier<TAB>path<TAB>kind" per line.
components() {
  local app="$1" bin="$1/Contents/Resources/bin" file framework_id
  [[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$app/Contents/Info.plist")" == "$app_id" ]] ||
    die "Whisper must keep CFBundleIdentifier $app_id."
  for file in "$bin"/*; do
    if [[ -f "$file" && ! -L "$file" ]] && file -b "$file" | grep -q '^Mach-O'; then
      printf '%s.%s\t%s\thelper\n' "$app_id" "$(basename "$file")" "$file"
    fi
  done
  if [[ -d "$bin/MediaRemoteAdapter.framework" ]]; then
    framework_id="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' \
      "$bin/MediaRemoteAdapter.framework/Resources/Info.plist")"
    printf '%s\t%s\tframework\n' "$framework_id" "$bin/MediaRemoteAdapter.framework"
  fi
  printf '%s\t%s\tapp\n' "$app_id" "$app"
}

verify() {
  local app="$1" fp id path kind expected actual
  fp="$(fingerprint)"
  codesign --verify --deep --strict "$app" || die "Signature check failed for $app."
  while IFS=$'\t' read -r id path kind; do
    expected="$(requirement "$id" "$fp")"
    codesign --verify --strict -R "=$expected" "$path" ||
      die "$(basename "$path") does not satisfy its release requirement."
    actual="$(codesign --display --requirements - "$path" 2>/dev/null | sed -n 's/^designated => //p')"
    if [[ "$(normalize <<<"$actual")" != "$(normalize <<<"$expected")" || "$actual" == *cdhash* ]]; then
      die "Unstable designated requirement for $(basename "$path")."
    fi
  done < <(components "$app")
  echo "[sign] Verified release signatures against the pinned certificate."
}

sign() {
  local app="$1" fp identity password work keychain keychain_password
  fp="$(fingerprint)"

  if [[ -n "${WHISPER_SIGNING_CERTIFICATE+set}" || -n "${WHISPER_SIGNING_PASSWORD+set}" ]]; then
    [[ -n "${WHISPER_SIGNING_CERTIFICATE:-}" && -n "${WHISPER_SIGNING_PASSWORD:-}" ]] ||
      die "Set both WHISPER_SIGNING_CERTIFICATE and WHISPER_SIGNING_PASSWORD."
    password="$WHISPER_SIGNING_PASSWORD"
  else
    [[ -s "$signing_dir/identity.p12" && -s "$signing_dir/password" ]] ||
      die "Signing identity is missing. Restore the Whisper signing backup or set the release signing secrets."
    password="$(<"$signing_dir/password")"
  fi

  work="$(cd "$(mktemp -d "${TMPDIR:-/tmp}/whisper-signing.XXXXXX")" && pwd -P)"
  keychain="$work/signing.keychain-db"
  keychain_password="$(openssl rand -base64 32)"
  identity="$work/identity.p12"
  # shellcheck disable=SC2064
  trap "cleanup '$work' '$keychain'" EXIT

  (
    umask 077
    if [[ -n "${WHISPER_SIGNING_CERTIFICATE:-}" ]]; then
      printf '%s' "$WHISPER_SIGNING_CERTIFICATE" | tr -d '[:space:]' | base64 -D >"$identity" 2>/dev/null ||
        die "WHISPER_SIGNING_CERTIFICATE must contain a base64-encoded PKCS#12 file."
    else
      cp "$signing_dir/identity.p12" "$identity"
    fi
  )
  [[ -s "$identity" ]] || die "WHISPER_SIGNING_CERTIFICATE must contain a base64-encoded PKCS#12 file."

  security create-keychain -p "$keychain_password" "$keychain" >/dev/null || die "Creating temporary signing keychain failed."
  # codesign --keychain narrows identity lookup but still uses the search list.
  search_list_add "$keychain"
  security unlock-keychain -p "$keychain_password" "$keychain" >/dev/null || die "Unlocking signing keychain failed."
  security set-keychain-settings -lut 3600 "$keychain" >/dev/null
  # security errors can echo arguments, which include the password.
  security import "$identity" -k "$keychain" -P "$password" -T /usr/bin/codesign >/dev/null 2>&1 ||
    die "Importing signing identity failed."
  security set-key-partition-list -S apple-tool:,apple: -s -k "$keychain_password" "$keychain" >/dev/null 2>&1 ||
    die "Configuring signing key access failed."
  # No -v: a self-signed identity need not be trusted system-wide.
  security find-identity -p codesigning "$keychain" | grep -qi ") $fp \"" ||
    die "The imported signing identity does not match the pinned certificate or has no private key."

  local id path kind options
  while IFS=$'\t' read -r id path kind; do
    options=()
    case "$kind" in
      helper) options=(--options runtime) ;;
      app) options=(--options runtime --entitlements "$entitlements") ;;
    esac
    codesign --force --sign "$fp" --keychain "$keychain" --timestamp=none \
      --identifier "$id" --requirements "=designated => $(requirement "$id" "$fp")" \
      ${options[@]+"${options[@]}"} "$path" ||
      die "Signing $(basename "$path") failed."
  done < <(components "$app")

  verify "$app"
}

search_list() {
  security list-keychains -d user | sed -n 's/^[[:space:]]*"\(.*\)"[[:space:]]*$/\1/p'
}

search_list_add() {
  local current=()
  while IFS= read -r entry; do current+=("$entry"); done < <(search_list)
  security list-keychains -d user -s ${current[@]+"${current[@]}"} "$1"
}

cleanup() {
  local work="$1" keychain="$2" remaining=()
  if [[ -e "$keychain" ]]; then
    security delete-keychain "$keychain" >/dev/null 2>&1 || true
  fi
  while IFS= read -r entry; do
    [[ "$entry" == "$keychain" ]] || remaining+=("$entry")
  done < <(search_list)
  security list-keychains -d user -s ${remaining[@]+"${remaining[@]}"} || true
  rm -rf "$work"
}

[[ "$(uname -s)" == Darwin ]] || die "Whisper release signing requires macOS."
case "${1:-}" in
  sign | verify)
    [[ -d "${2:-}" ]] || die "Usage: $0 sign|verify /path/to/Whisper.app"
    "$1" "$(cd "$2" && pwd -P)"
    ;;
  *) die "Usage: $0 sign|verify /path/to/Whisper.app" ;;
esac
