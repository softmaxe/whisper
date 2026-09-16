# Network access

Allow the hostname or IP address and port of each Self-Hosted server configured in Settings. Whisper has no fixed provider-domain list.

| Request                   | Destination                | Data sent                                    |
| ------------------------- | -------------------------- | -------------------------------------------- |
| Dictation and Upload ASR   | Settings > Speech-to-Text   | Recorded or selected audio                   |
| Clean Up and prompt tests | Settings > Language Models | Transcription text and the configured prompt |

Localhost, LAN, and remote URLs are supported. The URL determines the protocol and port. HTTP connections are unencrypted.

Homebrew and GitHub Releases downloads require access to those services.

## Connection checks

Use the server's health or model endpoint to test connectivity. A response confirms reachability; transcription still requires a valid model, request format, and credentials.

- `ENOTFOUND`: check the hostname and DNS.
- `ECONNREFUSED`: check that the server is listening on the configured address and port.
- `ETIMEDOUT`: check server availability, routing, and firewall rules.
- TLS certificate errors: check the server certificate and trusted certificate authorities.

Keep private URLs and credentials out of public diagnostics. See [Debug logging](../DEBUG.md).
