# Whisper tests

The regression suite follows the supported features in the root README. Keep inherited tests when they protect a feature used by this fork. Remove suites for retired features instead of hiding them with skips or a separate test command.

## Run

```sh
npm ci
npm test
npm run quality-check
```

Use Node.js 24 to install dependencies and run npm. `npm test` runs the suite inside Electron's Node runtime, matching the SQLite binding built by `npm ci`. Database tests must run; an unavailable native binding fails the command. `quality-check` adds lint, TypeScript, and translation checks and is the same command used by the Build workflow.

## Coverage

| Product behavior            | Regression coverage                                                                                                                                     |
| --------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Dictation                   | Recording start, stop and cancellation; microphone selection and recovery; MacBook lid changes; shortcuts; recording pill; clipboard and paste outcomes |
| Self-hosted processing      | ASR endpoint configuration, request authentication, file transport, cleanup prompts, request parameters, timeouts and failure recovery                  |
| Local History               | Transcript and audio retention, discarded recordings, retry, and database persistence                                                                   |
| Dictionary and Snippets     | Dictionary persistence and import, correction learning, text matching, and snippet expansion                                                            |
| File uploads                | Supported formats, batch state, cancellation, raw transcript persistence, and copyable results when History is disabled                                 |
| Local Insights and settings | Local event accounting, date grouping, microphone settings, menu bar visibility, permissions, and translations                                          |

OpenWhispr Cloud, accounts, sync, enterprise policies, meetings, Notes, Assistant, bundled model servers, Linux, and Windows are outside this suite. Shared test doubles may still name upstream interfaces imported by the retained implementation; those names do not add product requirements.

The Build workflow also packages the macOS ARM64 app and checks its bundle name, version, architecture, signature, archive, and checksum. Release reuses that build before publishing to GitHub and updating the Homebrew tap.

## Release smoke check

Automated checks do not establish microphone, Accessibility, or server compatibility on a user's Mac. Before calling a release ready, check the packaged app on macOS:

- Configure the intended ASR and cleanup servers, dictate with the global shortcut, and verify the pasted result and cleanup failure fallback.
- Switch microphones, exercise Auto selection with the laptop lid, and verify recording cancellation.
- Retry a History item, then check copy, delete, and retention settings.
- Upload a file and a batch with History both enabled and disabled. Check raw results remain copyable and do not increase dictation Insights.
- Verify dictionary and snippet behavior, the menu bar toggle, and installation through the Homebrew cask.
