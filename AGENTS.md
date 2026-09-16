# Project maintenance

## Changes and validation

- Keep changes within the scope in [README.md](README.md). For behavior or test changes, follow [test/README.md](test/README.md).
- Before fixing a problem or adding a feature, inspect upstream OpenWhispr. Reuse its code or adapt it with minimal changes before writing a new implementation.
- Use the Node.js version in `.nvmrc` and install dependencies with `npm ci`. Run `npm run quality-check` for code changes. Documentation-only changes need formatting and link checks.
- Treat the repository and release artifacts as public. Use portable paths and secret placeholders; keep credentials out of source, logs, and artifacts. The pinned `resources/mac/signing-certificate.pem` must contain only the public certificate.

## Signing and releases

- Before signing setup, backup or recovery, signing or packaging changes, releases, or investigating repeated macOS authorization prompts, read [docs/macos-signing.md](docs/macos-signing.md).
- Reuse the original certificate and private key. Preserve app and helper identifiers, certificate-bound designated requirements, and Keychain-backed secret storage. If the original signing credentials are unavailable or do not match the pinned certificate, stop release packaging and report the problem. Restore credentials instead of generating a replacement identity or falling back to ad-hoc signing.
- Before changing CI or the release process, read [package.json](package.json), [Build](.github/workflows/build.yml), and [Release](.github/workflows/release.yml). Pass named secrets only to trusted release jobs; keep PR builds credential-free.
- After signing or packaging changes, run `npm run test:signing` and `npm run pack:release` on macOS. Verify temporary private files and keychains are cleaned up on success and failure.
- Publish only when requested. Before reporting a release complete, verify all Release workflow jobs, the published archive and checksum, and the Homebrew cask version.
- For release readiness or permission-retention validation, follow the [release smoke check](test/README.md#release-smoke-check). Report automated signature checks separately from actual microphone, Accessibility, and Keychain behavior after a Homebrew upgrade.
