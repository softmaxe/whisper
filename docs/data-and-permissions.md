# Data and permissions

## Network access

Allow the hostname or IP address and port of each server configured in Settings. Whisper has no fixed provider-domain list.

| Request                   | Destination               | Data sent                                    |
| ------------------------- | ------------------------- | -------------------------------------------- |
| Dictation and Upload ASR  | Settings > Speech-to-Text | Recorded or selected audio                   |
| Clean Up and prompt tests | Settings > Text cleanup   | Transcription text and the configured prompt |

Processing stays on the device only when those servers run locally. HTTP is allowed for loopback and private-network hosts, including `.local` and `.ts.net` names. Public hosts require HTTPS. HTTP connections are unencrypted.

Homebrew and GitHub Releases downloads contact those services. The app has no automatic updater.

## Local data and permissions

Microphone access enables recording. Accessibility access enables automatic paste and correction monitoring.

History, Dictionary, Snippets, and settings are stored in `~/Library/Application Support/whisper/`, separately from OpenWhispr. Dictation audio follows the retention setting; Upload does not retain source audio.

Saved secrets are encrypted under `secure-keys/`, using the `whisper` Keychain namespace or Electron `safeStorage`. Review logs and exported transcripts before sharing.

macOS associates permission grants and Keychain access with the app's code identity. Older releases used ad-hoc signatures that changed with each build, so an upgrade could require permission again. Release builds now use the same self-signed certificate and stable bundle identifiers. The first upgrade to this identity may still require microphone, Accessibility, and Keychain access once more. Permission retention across later upgrades must be checked on a real Mac, as described in the [release smoke check](../test/README.md#release-smoke-check).

The Keychain dialog for `whisper` concerns access to saved secrets. "Always Allow" grants access to the current app identity. Keeping the release identity stable lets macOS recognize later builds; clicking it on an older ad-hoc build cannot preserve that build's identity across an update. Release signing does not reset system permissions or delete Keychain items. See [macOS signing](macos-signing.md) for the signing and backup process.

## Connection problems

Test the server's health or model endpoint from the Mac running Whisper. A response confirms reachability; transcription still requires the expected request format and a valid model.

- `ENOTFOUND`: check the hostname and DNS.
- `ECONNREFUSED`: check that the server is listening on the configured address and port.
- `ETIMEDOUT`: check server availability, routing, and firewall rules.
- TLS certificate errors: check the server certificate and trusted certificate authorities.

For detailed logs, quit Whisper and launch it with:

```sh
open -a Whisper --args --log-level=debug
```

Reproduce the issue, then inspect `~/Library/Application Support/whisper/logs/`. Quit and relaunch normally to stop debug logging. Remove private URLs, transcripts, and credentials before sharing diagnostics.
