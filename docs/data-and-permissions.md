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
