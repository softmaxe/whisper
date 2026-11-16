# Live dictation

Users record speech with a global shortcut or menu-bar action, send it to ASR and optionally cleanup, then receive pasted text or a copy fallback in their target application.

## Sub-features

- `dictation-tap-hold`: double-tap and hold shortcut activation.
- `dictation-menu`: start/stop through the menu bar.
- `dictation-finish-cancel`: actual recording finish/cancel and pill feedback.
- `dictation-cleanup-delivery`: cleanup/skip/fallback and text delivery.
- `dictation-dictionary`: spoken terms/snippets and usage updates.

## How to get to it (user POV)

- Default empty configuration uses Globe: double-tap to begin hands-free, tap to finish. In hold mode press/release the configured shortcut.
- Menu-bar action `Start listening` / `Stop listening`.
- Recording pill buttons `Finish Dictation`, `Cancel Dictation`, and processing `Skip cleanup`; Escape cancels recording.

## Driving it with cua_repl

Preconditions:

- Use a dedicated macOS test account/session with no other Whisper/global-shortcut driver, actual microphone and Accessibility authorization, configured ASR, and a disposable target editor.
- The standard helper's `home` scene intentionally disables global hotkeys. It cannot prove shortcut activation. A real dictation run needs an explicitly owned launch without `WHISPER_DEV_SCENE`, after excluding other instances; record its exact executable/PID/data directory and provide equivalent Doctor/cleanup ownership checks. Do not quit the user's installed session without authorization.
- Use synthetic/disposable spoken content and a known endpoint; cleanup needs its own configured model/server. Do not obtain or expose shared Keychain credentials for this proof.

- **Target.** Bind the disposable editor with `cua.getApp`, focus its editable document, and capture the empty target. Resolve app/menu/pill AX targets separately; don't send dictation input into a real user's document.
- **Shortcut.** Through CUA send the configured key press/release sequence; for held keyboard keys use the documented `durationMs` press option. Require an actual live recording pill/timer and the active microphone. If CUA cannot synthesize Globe on this host, report that entry blocked; another shortcut/menu path does not certify Globe.
- **Menu/pill.** Independently choose `Start listening` from this run's menu-bar entry, speak the known sentence, then click `Finish Dictation` or choose `Stop listening`. Capture the recording action, ASR/cleanup progress, and delivered text. On a separate recording click `Cancel Dictation`/Escape; require no delivered text and verify stored side effects.
- **Delivery.** Inspect the target editor for the expected sentence and query History/audio state. When automatic paste fails, capture the actual fallback and paste the copied text into the disposable target. Exercise `Skip cleanup` during an actual pending cleanup request, not a dev-scene animation.
- **Dictionary.** Create a word/snippet through the mapped Dictionary UI, dictate its spoken form, and verify output/usage. Preserve the real audio fixture or spoken-content transcript and server response evidence where available, using disposable content only.

## Gotchas

- The baseline deliberately lacks server configuration, microphone authorization, a target editor, and registered hotkeys: live dictation fails its prerequisites there. This feature was not established by the generated skill's offline proof.
- Data-directory isolation alone permits duplicate recording/paste from global listeners. Do not launch a second live shortcut instance beside the installed app.
- `WHISPER_DEV_SCENE=pill:*` changes internal presentation state; its screenshots never prove capture, upload, cleanup, permission behavior, or paste.
- Real microphone/Accessibility/Keychain retention after a Homebrew upgrade belongs to `docs/testing.md`'s release smoke check and `docs/macos-signing.md`; a debug verification run cannot establish it.
