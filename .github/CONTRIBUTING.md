# Contributing to whisper

Read the supported feature scope in [README.md](../README.md) before changing behavior. This project reuses OpenWhispr's self-hosted dictation implementation and UI. Keep changes focused on that scope and preserve the original interaction patterns.

Use Node.js 24 from `.nvmrc`. Install with `npm ci`, then run `npm run dev`. Run `npm run quality-check`, `npm run i18n:check`, and the native Electron test command documented in the README before submitting changes.

Report bugs and propose changes in [this repository's issues](https://github.com/softmaxe/whisper/issues). Include the application version, macOS version, reproduction steps, and relevant redacted errors. Keep credentials, private server addresses, prompts, and recordings out of public reports.

The upstream code remains MIT-licensed. Preserve its license and attribution when reusing or modifying it.
