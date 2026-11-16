# Settings

Users configure appearance/language, dictation controls, speech and cleanup servers, microphone selection, and local data retention.

## Sub-features

- `settings-entry`: sidebar, app menu, Cmd+comma; close and restore focus.
- `settings-appearance`: theme/language change visibly and persist.
- `settings-servers`: actual connection tests, model selection, optional cleanup.
- `settings-dictation-privacy`: microphone/test, controls and retention behavior.

## How to get to it (user POV)

- Sidebar `Settings`, `Cmd+,`, or app menu `Whisper` > `Settings…`.
- Sections `General`, `Dictation`, `Speech to text`, `Text cleanup`, `Privacy & data`.
- Upload's `Speech to Text Settings…` and Dictionary's `Dictionary Settings…` are contextual entries.
- Close with `Close settings`, Escape, or the visible backdrop.

## Driving it with cua_repl

Preconditions:

- A healthy isolated instance; understand the shared Keychain limitation in the parent skill before opening Settings. Do not authorize a Keychain prompt or edit a stored API key as part of routine verification.
- Server paths additionally need a disposable reachable endpoint/model; microphone paths need actual macOS permission and hardware in a suitable test session.

- **Entry/close.** `await app.pressKey("super+comma")`, inspect AX and require the sheet/section names. Close with `await app.pressKey("Escape")` and check focus returns to the previous page. Repeat through sidebar, menu, and relevant contextual entries to claim their coverage.
- **Appearance.** Click `General`, then `Dark`; inspect AX and capture the rendered change. Read `scratch/data/settings.json` and require `theme` equal to `dark`. Repeat `Light`, `Tokyo Night`, and `Auto` only as required, using current indices. Change `Interface language` and require the actual translated navigation, not just a saved value. Record which language subsequent selectors use.
- **Server.** Click `Speech to text`, fill the visible `Server URL` and `Model` inputs with the disposable endpoint/model, choose `Test Connection`, and capture success or specific failure. Repeat cleanup under `Text cleanup` where needed; never record API key content. Then prove a real Upload/dictation journey using that configuration.
- **Controls/privacy.** Open `Dictation` to select the microphone and exercise its actual test recording/playback; open `Privacy & data` to change `Save history`/retention only within isolated data and prove the resulting History/audio behavior through a new real transcription.
- **Stored proof.** Read only the relevant nonsecret `settings.json` keys, close/reopen Settings and verify selection/rendering. Report restart persistence separately; the initial Dictionary proof does not restart the process.

## Gotchas

- `WHISPER_USER_DATA_DIR` does not isolate Keychain, Accessibility/microphone permission, login items, media controls, or system preferences. Avoid Start at login and permission resets in routine runs.
- Constructing Settings reads the fixed cleanup Keychain entry even before choosing Text cleanup. If this triggers authorization, stop that journey and retain the blocker evidence.
- No server or microphone authorization exists in the baseline. Never downgrade failed connection/recording prerequisites into a pass for opening the sheet.
- Menus are localized and dropdown queries reset when reopened. Resolve fresh accessible names after a language change.
