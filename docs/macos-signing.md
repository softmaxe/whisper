# macOS signing

Release builds use one persistent self-signed code signing certificate. This is a free option that does not require an Apple Developer account. Development builds made with `npm run pack` continue to use ad-hoc signing.

An ad-hoc signature identifies a particular build by its code hash. Rebuilding the app changes that identity, which can trigger new microphone, Accessibility, or Keychain authorization. The release signing requirement binds each bundle identifier to the pinned certificate instead. Keeping the certificate, private key, and bundle identifiers unchanged gives macOS a stable identity across releases. See Apple's [code signing requirements](https://developer.apple.com/documentation/technotes/tn3127-inside-code-signing-requirements) and [code signing guidance](https://developer.apple.com/library/archive/technotes/tn2206/).

Switching from an ad-hoc build may require authorization once more. Permission retention after that transition needs the [two-version upgrade smoke check](../test/README.md#release-smoke-check). Self-signing does not provide Developer ID verification or notarization, so Gatekeeper may still warn about an unidentified developer.

## Create the identity once

The initial maintainer creates the identity on macOS:

```sh
npm run signing:create
```

The command creates these files:

| File                                     | Purpose                                                                            |
| ---------------------------------------- | ---------------------------------------------------------------------------------- |
| `resources/mac/signing-certificate.pem`  | Public certificate pinned in the repository and used to verify release signatures. |
| `~/.config/whisper/signing/identity.p12` | Encrypted certificate and private key used to sign releases. Keep it private.      |
| `~/.config/whisper/signing/password`     | Password for the encrypted identity. Keep it private.                              |

The command refuses to overwrite an existing public certificate or local identity backup. Do not regenerate the identity for a new version, a new checkout, or a replacement Mac. Restore the original `identity.p12` and `password` files instead. A new certificate changes the release identity and may require users to authorize the app again.

Keep an encrypted backup of both private files outside the repository, with access limited to release maintainers. The public certificate is safe to commit; the private key, PKCS#12 file, and password are not. If the private key is lost, the public certificate cannot recover it or sign another release.

## Build locally

```sh
npm run pack:release
```

The default package is the native arm64 application for macOS 27. It writes
`dist/native-arm64/Whisper.app`, `release/whisper-VERSION-macos-arm64.zip` and its
`.sha256` file. Packaging stages and verifies the bundle and extracted archive
before replacing those exact outputs; a failure preserves previous artifacts and
removes temporary staging material. Existing legacy baseline archives are retained.

The release build reads the saved identity and password from `~/.config/whisper/signing/`. It signs the app, helpers, and bundled executable code, then checks the result against the pinned public certificate. Missing or mismatched credentials fail the build instead of producing an ad-hoc signed release.

To supply credentials from a secret store, set both environment variables:

| Variable                      | Value                                                   |
| ----------------------------- | ------------------------------------------------------- |
| `WHISPER_SIGNING_CERTIFICATE` | Base64-encoded contents of the original `identity.p12`. |
| `WHISPER_SIGNING_PASSWORD`    | Password for that PKCS#12 file.                         |

Local files are used only when both environment variables are absent. Setting just one is an error. Avoid putting either value in shell history, logs, source files, or committed configuration.

`npm run pack` builds the native app without reading any release credentials,
even if signing environment variables or local backups exist. It writes
`dist/native-development-arm64/Whisper.app` and an explicitly named
`-development.zip` archive. Installing an ad-hoc development build over a release
can change its identity; use release builds for permission-retention testing.
`npm run legacy:pack:release` remains available for legacy baseline comparisons.

On macOS, check that changed app and native helper binaries keep the same signing identity:

```sh
npm run test:signing
```

This test requires the original signing credentials, using the same local files or environment variables as the release build. It signs two temporary app versions with different code, compares their designated requirements, and checks that each version satisfies the other's requirement. It also checks that a native helper cannot satisfy the main app's requirement. The test also checks that temporary private files and Keychain entries disappear
after successful signing and a failed PKCS12 import. The test does not install an app or request permissions, so it cannot replace the [upgrade smoke check](../test/README.md#release-smoke-check).

## Configure GitHub releases

Store the original identity in repository Actions secrets named `WHISPER_SIGNING_CERTIFICATE` and `WHISPER_SIGNING_PASSWORD`. With GitHub CLI authenticated for this repository, upload the local files without printing their contents:

```sh
base64 -i "$HOME/.config/whisper/signing/identity.p12" | gh secret set WHISPER_SIGNING_CERTIFICATE
gh secret set WHISPER_SIGNING_PASSWORD < "$HOME/.config/whisper/signing/password"
```

The Release workflow passes these named secrets to the reusable Build workflow for tagged releases and runs `npm run test:signing` before packaging. Pull requests and pushes to `main` run native tests and development packaging
without signing credentials. Manual Build runs produce ad-hoc signed packages without release credentials. Release signing imports the identity into a temporary build keychain, verifies signatures against `resources/mac/signing-certificate.pem`, and cleans up its temporary keychain. A release cannot proceed with a different certificate or an ad-hoc signature.

Restore these same secret values when moving the release workflow to another repository. Do not generate a fresh certificate on each runner or release. The private key is needed only to build releases; users do not need the signing files.

## Restore or remove local credentials

On another Mac, restore the original `identity.p12` and `password` into `~/.config/whisper/signing/`. Restrict the directory to its owner and both files to owner read/write access. The files must match the repository's pinned public certificate.

After confirming a secure backup and working release credentials, remove the local copies if they are no longer needed:

```sh
rm -rf "$HOME/.config/whisper/signing"
```

This prevents local release builds until the credentials are restored or supplied through the environment. It does not revoke signatures on existing releases. The signing process does not change system trust, reset TCC permissions, or delete Keychain items.
