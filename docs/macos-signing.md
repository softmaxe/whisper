# macOS signing

Release builds use one persistent self-signed code signing certificate. This is a free option that does not require an Apple Developer account. Development builds made with `scripts/bundle-macos.sh` continue to use ad-hoc signing.

An ad-hoc signature identifies a particular build by its code hash. Rebuilding the app changes that identity, which can trigger new microphone, Accessibility, or Keychain authorization. The release signing requirement binds each identifier to the pinned certificate instead. Keeping the certificate, private key, and identifiers unchanged gives macOS a stable identity across releases. See Apple's [code signing requirements](https://developer.apple.com/documentation/technotes/tn3127-inside-code-signing-requirements) and [code signing guidance](https://developer.apple.com/library/archive/technotes/tn2206/).

Self-signing does not provide Developer ID verification or notarization, so Gatekeeper may still warn about an unidentified developer.

## Identity

| File                                     | Purpose                                                                            |
| ---------------------------------------- | ---------------------------------------------------------------------------------- |
| `packaging/signing-certificate.pem`      | Public certificate pinned in the repository and used to verify release signatures. |
| `~/.config/whisper/signing/identity.p12` | Encrypted certificate and private key used to sign releases. Keep it private.      |
| `~/.config/whisper/signing/password`     | Password for the encrypted identity. Keep it private.                              |

Do not regenerate the identity for a new version, a new checkout, or a replacement Mac. Restore the original `identity.p12` and `password` files instead. A new certificate changes the release identity and may require users to authorize the app again.

Keep an encrypted backup of both private files outside the repository, with access limited to release maintainers. The public certificate is safe to commit; the private key, PKCS#12 file, and password are not. If the private key is lost, the public certificate cannot recover it or sign another release.

## Signed components

[`scripts/sign-macos-release.sh`](../scripts/sign-macos-release.sh) signs nested code first, then the app. Each component gets the designated requirement `identifier "<id>" and certificate leaf = H"<certificate SHA-1>"`:

| Component                                  | Identifier                                        |
| ------------------------------------------ | ------------------------------------------------- |
| `Whisper.app`                              | `com.softmaxe.whisper` (its `CFBundleIdentifier`) |
| Mach-O executables in `Resources/bin`      | `com.softmaxe.whisper.<file name>`                |
| `Resources/bin/MediaRemoteAdapter.framework` | the framework's `CFBundleIdentifier`            |

Keep these identifiers unchanged. Renaming a helper changes its identity.

## Build locally

```sh
scripts/download-ffmpeg.sh
scripts/bundle-macos.sh --release
```

The release build reads the saved identity and password from `~/.config/whisper/signing/`. It imports them into a temporary keychain, signs the app, helpers, and framework, checks every designated requirement against the pinned public certificate, and deletes the temporary keychain on success or failure. Missing or mismatched credentials fail the build instead of producing an ad-hoc signed release.

To supply credentials from a secret store, set both environment variables:

| Variable                      | Value                                                   |
| ----------------------------- | ------------------------------------------------------- |
| `WHISPER_SIGNING_CERTIFICATE` | Base64-encoded contents of the original `identity.p12`. |
| `WHISPER_SIGNING_PASSWORD`    | Password for that PKCS#12 file.                         |

Local files are used only when both environment variables are absent. Setting just one is an error. Avoid putting either value in shell history, logs, source files, or committed configuration.

To check an already signed app against the pinned certificate:

```sh
scripts/sign-macos-release.sh verify dist/Whisper.app
```

## Configure GitHub releases

Store the original identity as secrets of the `release` Actions environment, named `WHISPER_SIGNING_CERTIFICATE` and `WHISPER_SIGNING_PASSWORD`. Store the Homebrew tap token there too, as `TAP_GITHUB_TOKEN`. The environment only accepts deployments from `v*` tags, so workflows on branches and pull requests cannot read these secrets. Do not keep copies as repository secrets. With GitHub CLI authenticated for this repository, upload the local files without printing their contents:

```sh
base64 -i "$HOME/.config/whisper/signing/identity.p12" | gh secret set WHISPER_SIGNING_CERTIFICATE --env release
gh secret set WHISPER_SIGNING_PASSWORD --env release < "$HOME/.config/whisper/signing/password"
gh secret set TAP_GITHUB_TOKEN --env release
```

Scope `TAP_GITHUB_TOKEN` to the Homebrew tap repository with contents write access only, and give it an expiration date.

The Release workflow requires the tag to point at a commit on `main`. Its Build call uses the `release` environment for tagged releases. Pull requests and pushes to `main` run checks without packaging or signing credentials. Manual Build runs produce ad-hoc signed packages without release credentials. A release cannot proceed with a different certificate or an ad-hoc signature.

Restore these same secret values when moving the release workflow to another repository. Do not generate a fresh certificate on each runner or release. The private key is needed only to build releases; users do not need the signing files.

## Restore or remove local credentials

On another Mac, restore the original `identity.p12` and `password` into `~/.config/whisper/signing/`. Restrict the directory to its owner and both files to owner read/write access. The files must match the repository's pinned public certificate.

After confirming a secure backup and working release credentials, remove the local copies if they are no longer needed:

```sh
rm -rf "$HOME/.config/whisper/signing"
```

This prevents local release builds until the credentials are restored or supplied through the environment. It does not revoke signatures on existing releases. The signing process does not change system trust, reset TCC permissions, or delete Keychain items.
