# Data and permissions

## Data and network access

ASR sends audio to the configured Self-Hosted server. Clean Up sends transcription text and the configured prompt. Processing stays on the device only when those servers run locally.

HTTP URLs are supported and do not provide TLS encryption. See [Network access](network-allowlist.md).

History, Dictionary, Snippets, and settings are stored in `~/Library/Application Support/whisper/`. Dictation audio follows the retention setting; Upload does not retain source audio.

Saved secrets are encrypted under `secure-keys/`, using the `whisper` Keychain namespace or Electron `safeStorage`. Review logs and exported transcripts before sharing.

## Application boundary

The renderer uses context isolation with Node integration disabled. Native helpers handle shortcuts, correction monitoring, and paste. Releases use ad-hoc signing and are not notarized. Install updates through Homebrew or GitHub Releases.
