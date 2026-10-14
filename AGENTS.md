# macOS signing

- Before signing setup, backup or recovery, signing or packaging changes, releases, or investigating repeated macOS authorization prompts, read [docs/macos-signing.md](docs/macos-signing.md).
- Reuse the original certificate and private key. Preserve the app and helper identifiers, certificate-bound designated requirements, and Keychain-backed secret storage. If the original signing credentials are unavailable or do not match the pinned certificate, stop release packaging and report the problem. Restore credentials instead of generating a replacement identity or falling back to ad-hoc signing.
- The pinned `packaging/signing-certificate.pem` must contain only the public certificate. Keep the PKCS#12 file and its password out of source, logs, and artifacts, and pass the signing secrets only to trusted release jobs; keep PR builds credential-free.
- After signing or packaging changes, run `scripts/bundle-macos.sh --release` on macOS. Verify temporary private files and keychains are cleaned up on success and failure.
- For permission-retention validation, follow the [release smoke check](docs/testing.md#release-smoke-check). Report automated signature checks separately from actual microphone, Accessibility, and Keychain behavior after a Homebrew upgrade.

# Releases

- When asked to release without a version, bump the patch number by one (`0.0.2` → `0.0.3`) in `Cargo.toml` and `Cargo.lock`, merge it to `main`, then push the `v<version>` tag that triggers the Release workflow.
- The release is done once the workflow's Homebrew tap job has pushed the new cask and the local install is upgraded with `brew update && brew upgrade --cask whisper`, confirmed by `brew list --cask --versions whisper`.
