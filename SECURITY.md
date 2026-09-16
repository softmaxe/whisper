# Security

This document describes the macOS Apple Silicon build in this repository. For its supported features and releases, see [README.md](README.md).

## Data and network access

ASR sends recorded or uploaded audio to the configured Self-Hosted server. Clean Up sends transcription text and the configured prompt to its selected server. These servers may run locally, on a LAN, or remotely; Self-Hosted does not mean that processing stays on the device.

Server URLs determine the transport. HTTP URLs are supported and do not provide TLS encryption. See [Network access](docs/network-allowlist.md).

History, Dictionary, Snippets, and settings are local to `~/Library/Application Support/whisper/`. Dictation audio follows the configured retention settings. Upload does not retain a copy of its source audio.

Saved secrets use the existing encrypted credential store under `secure-keys/`, with the `whisper` macOS Keychain namespace and Electron `safeStorage` fallback. Logs and exported transcripts can contain personal data; review them before sharing.

## Application boundary

The renderer uses context isolation with Node integration disabled. Native helpers provide shortcuts, correction monitoring, and paste. Releases use ad-hoc signing and are not notarized. Updates are installed through Homebrew or GitHub Releases; the upstream in-app updater is disabled.

## Reporting

Report vulnerabilities in this fork privately to its repository owner. The [repository Security page](https://github.com/softmaxe/whisper/security) lists available reporting options. Do not include credentials, personal recordings, or exploit details in public issues. The original OpenWhispr project's support addresses and response-time commitments do not apply to this fork.
