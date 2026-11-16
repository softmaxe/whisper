# Whisper verification map

This is the maintained source for user-facing verification. Read the parent [skill](../SKILL.md) for launch, ownership, CUA, evidence, and cleanup instructions.

## Baseline preconditions

- Use a new helper-owned debug instance with empty isolated data, English UI, and `WHISPER_DEV_SCENE=home`.
- Require Doctor success and the actual rendered Whisper control panel before driving it.
- There are no transcripts, words, snippets, analytics events, ASR/cleanup endpoints, or supplied credentials. The offline Dictionary journey creates its own data through the UI.
- Native Accessibility tooling must be available. Bind by the helper's exact app bundle path, never the installed app's name or bundle ID.
- The default panel-only scene disables global hotkeys; it is unsuitable for a keyboard dictation proof.

## Driving conventions

- All feature recipes use `cua_repl` and the `app` binding from Launch. Resolve `index` from a fresh `await app.getAXState()` by the stated accessible name; the recipes do not specify fixed indices.
- `await app.click(index)` activates that named element. `await app.typeText(text)` types into the focused input; `await app.pressKey(key)` sends real keyboard input. Inspect AX after each transition before choosing the next index. See the parent skill for initial Tab activation and Tab refresh after a stale AX-click result. Full/hidden AX snapshots can renumber indices; read a fresh emitted tree after saving evidence.
- If AX omits a target, save the screenshot and document a visual-coordinate fallback; do not invent an AX selector.
- Start from baseline unless the recipe states a stronger precondition. Create required rows through actual upload/dictation/UI actions; do not insert them in SQLite.

## Proof and failure reporting

- Save feature ID, entry point, action, resulting AX/screenshot, and read-only stored state under `$VERIFY_RUN`; preserve these through cleanup.
- A feature can have multiple entry points. Passing one never certifies another: report each as passed, failed with its unmet precondition, or not exercised.
- Missing audio, server, permission, or expected seed state is a failed journey precondition, not a successful empty-state proof. Record the reason in the matching feature's Gotchas when a new limitation is discovered.
- Initial generated-skill validation is intentionally one Dictionary journey. Other mapped paths are instructions for later coverage, not claims of execution.

## Features

- [Dictionary and snippets](dictionary.md): empty-state/header creation, import/export, filtering, edit/delete/undo, snippet replacement definitions.
- [History, search, and Insights](history.md): saved text, raw text, retry, deletion, search, dictation statistics.
- [File transcription](upload.md): chooser/drop, single/batch requests, cancellation, result and History.
- [Settings](settings.md): entry paths, appearance/language, server configuration, microphone/retention controls.
- [Live dictation](dictation.md): actual shortcut/menu start, finish/cancel, paste/fallback, cleanup.
