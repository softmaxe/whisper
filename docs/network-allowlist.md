# Network access

Whisper uses the Self-Hosted ASR and Clean Up URLs configured in Settings. Allow the hostname or IP address and port of each selected server. There is no fixed provider-domain list for these requests.

| Request                   | Destination                | Data sent                                    |
| ------------------------- | -------------------------- | -------------------------------------------- |
| Dictation and Upload ASR  | Settings > Speech-to-Text  | Recorded or selected audio                   |
| Clean Up and prompt tests | Settings > Language Models | Transcription text and the configured prompt |

Localhost, LAN, and remote URLs are supported. The URL selects HTTP or HTTPS and the port; requests are not necessarily TLS-encrypted. Use the server's actual configuration when setting firewall, proxy, or certificate rules.

This build does not start OpenWhispr account sign-in, cloud sync, streaming providers, local model downloads, calendar integration, URL imports, or in-app automatic updates. Installing or updating through Homebrew and GitHub Releases has separate network requirements.

## Connection checks

Confirm that the configured server is running and reachable from this Mac. Use its documented health or model endpoint to test connectivity. An HTTP response can confirm reachability, but does not establish that the model, request format, or credentials are valid.

- `ENOTFOUND`: inspect the hostname and DNS configuration.
- `ECONNREFUSED`: check that the server is listening on the configured address and port.
- `ETIMEDOUT`: inspect server availability, routing, and firewall rules.
- TLS certificate errors: check the certificate and trusted certificate authorities for the selected server.

Keep private URLs and credentials out of public diagnostics. See [Debug logging](../DEBUG.md).
