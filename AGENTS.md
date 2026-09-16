# Project maintenance

## Changes and validation

- When fixing a problem or adding a feature, inspect the upstream OpenWhispr code first. Prefer reusing it directly, then adapting it with minimal changes. Write a new implementation only when neither approach works.
- Keep changes within the supported product scope in [README.md](README.md). Read [test/README.md](test/README.md) when changing behavior or tests.
- Before editing, check the branch, working tree, remotes, and contribution rules. Preserve changes made by others. Commit, push, merge, tag, publish, or rewrite history only when the user requests that action.
- Use the Node.js version in `.nvmrc` and install dependencies with `npm ci`. Run `npm run quality-check` for code changes; use the additional signing checks below for packaging or signing changes. Documentation-only edits need formatting and link checks, not an application rebuild.
- Before an authorized commit, stage explicit paths and review the actual staged diff, including new files. Keep generated output and local credentials out of Git. Use English for code comments, documentation, commit messages, and PR text; use Conventional Commits without AI attribution.

## Public repository and secrets

- Treat source, documentation, examples, tests, issues, PRs, Actions logs, and release artifacts as public. Document secret names and placeholders, never secret values. Use portable paths instead of a maintainer's username or machine-specific absolute paths.
- Keep private signing keys, PKCS12 exports, their passwords, API keys, tokens, `.env` files, keychain files, and private backups outside the repository and packaged app. Base64 encoding and password protection do not make a private credential safe to publish. `.gitignore` does not protect files already tracked by Git.
- `resources/mac/signing-certificate.pem` is intentionally public and must contain only the signing certificate. Check the contents before adding or replacing a PEM file; a filename or extension does not establish that it contains no private key.
- Pass credentials through a secret store, stdin, or protected files. Avoid literal secret arguments, shell tracing, environment dumps, keychain dumps, and command errors that reproduce credentials. Report only names, fingerprints, paths, or redacted diagnostics.
- Before sharing a diff, uploading artifacts, or pushing an authorized change, inspect the exact files being shared for credentials and private data. If a leak is found, stop that publication and report its location without repeating the value. Resolve revocation or signing-key replacement with the maintainer; deleting the current file alone does not remove earlier exposure. Rewrite history only when explicitly authorized.

## Signing identity and recovery

- For certificate setup, backup, recovery, signing failures, packaging changes, or repeated macOS authorization prompts, read [docs/macos-signing.md](docs/macos-signing.md) before making changes. It is the source of truth for credential locations, secret names, and setup commands.
- Every release must reuse the original certificate and private key. Preserve `local.whisper.desktop`, helper identifiers, and certificate-bound designated requirements. Creating another certificate with the same name does not preserve identity.
- Restore existing credentials when changing machines, checkouts, or CI runners. Identity creation is a one-time bootstrap operation, not a build or upgrade step. If the backup is missing or does not match the pinned public certificate, stop release packaging and report the problem instead of generating a replacement or using ad-hoc signing.
- Keep an encrypted backup outside Git with access limited to maintainers. Delete a local signing backup only when requested and after confirming another recoverable copy. Handle a compromised key as an explicit identity migration, including the effect on existing permissions.
- Preserve temporary-keychain cleanup and existing system trust. Keep Keychain-backed secret storage. Do not work around upgrade prompts by resetting TCC, deleting saved secrets, weakening the signing requirement to a bundle identifier alone, or disabling signature verification.

## Packaging and release

1. Read the current `package.json` scripts, `.github/workflows/build.yml`, and `.github/workflows/release.yml` before changing the release process. For an authorized version bump, keep `package.json`, both version fields in `package-lock.json`, and the release tag `v<version>` consistent.
2. Use `npm run pack` for ad-hoc development packages and `npm run pack:release` for distributable releases. Release builds must fail on missing or mismatched credentials and verify the result against the pinned public certificate. Never publish a development artifact as a release.
3. On macOS, run `npm run test:signing` and `npm run pack:release` after signing or packaging changes. Preserve signing of the app and native helpers, stable identities for path aliases, and bundle sealing of non-code resources. Confirm that temporary private files and keychains are removed on success and failure.
4. Keep release credentials limited to trusted release jobs. Forward named secrets explicitly; PR builds must remain credential-free. Never execute untrusted PR code in a privileged workflow with signing or publishing secrets.
5. For an authorized release, follow the Release workflow through signature and archive checks, GitHub publication, and Homebrew tap synchronization. Verify all jobs and the published archive/checksum and cask version before reporting the release complete. Keep credentials, keychains, and backup files out of artifacts and logs.

## Permission verification and handoff

- Follow the two-version upgrade procedure in [test/README.md](test/README.md#release-smoke-check) when validating permission retention. Report automated signature checks separately from actual microphone, Accessibility, and Keychain behavior after a Homebrew upgrade.
- Moving from an ad-hoc build to the persistent identity may require authorization once more. Self-signing does not provide Apple notarization or remove Gatekeeper warnings.
- State what changed, which checks passed, any remaining verification gaps, and whether changes were committed, published, or installed. Report persistent local artifacts without exposing their contents, and clean up temporary files created during the task.
