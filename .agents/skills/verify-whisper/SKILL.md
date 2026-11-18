---
name: verify-whisper
description: Verify Whisper's macOS GPUI desktop app through native Accessibility input, with isolated data, process ownership checks, and persistent screenshots and database evidence. Use after user-facing behavior changes or to reproduce Dictionary, History, Upload, Settings, and dictation journeys.
---

# Verify Whisper

Whisper is an Apple Silicon macOS menu-bar dictation app with a GPUI control panel. Its primary surface is native UI; there is no browser, CDP, HTTP control API, or interactive CLI. Rust unit and service tests (`scripts/check.sh`) supplement real UI verification.

Read [features/README.md](features/README.md), then the requested feature. Commands below run from the checkout root. Python helpers use `uv`; no extra Python packages are needed.

## Launch

Requirements: an Apple Silicon Mac with a logged-in GUI session, the pinned Rust toolchain, Xcode command line tools, `uv` with Python 3.11+, and the `cua_repl` native-app tool. Check repository contribution rules before editing. This workflow uses the debug executable and a disposable app wrapper, without running release packaging, signing, or replacing the installed app. Read `docs/macos-signing.md` before extending this to signing or packaging changes.

```sh
cargo build --locked
VERIFY_RUN="$PWD/.e2e/verify-whisper/run-$(date +%Y%m%d-%H%M%S)"
uv run --no-project .agents/skills/verify-whisper/scripts/control.py launch --run-dir "$VERIFY_RUN"
```

Keep `VERIFY_RUN` for every subsequent command; use its absolute value in CUA file writes. If the shell does not retain variables, recover the chosen path from the launch invocation, not from whichever run is newest. Each launch requires a new run directory.

The helper copies the unchanged `target/debug/whisper` into `scratch/Whisper.app/Contents/MacOS/whisper` and starts it in its own process group with an **empty** `scratch/data` directory. The wrapper reuses `packaging/Info.plist`, its original identifiers, and the Cargo version. It is verification scaffolding because CUA rejects bare executables with `Invalid app`. It does not provide release-signature or permission-retention proof. Whisper performs its own SQLite migrations (currently `user_version=4`). No seed rows, server configuration, or credentials are installed. Default interface language is English; History saving is on; ASR and cleanup URLs/models are empty. Dictionary and snippets are usable offline from this state.

The helper sets `WHISPER_USER_DATA_DIR` and `WHISPER_HELPERS_DIR` explicitly and clears inherited `WHISPER_*` overrides. It sets `WHISPER_DEV_SCENE=home`: in `src/app.rs` this selects the ordinary History view and skips `start_hotkeys`. It injects no Dictionary or History records. Do not use `pill:*` scenes to claim real recording or processing behavior.

Launch waits for the owned PID, source/copied binary hashes, lock PID, migrated DB, and `retention cleanup:` log message. `ready-for-ui-check` still requires a rendered UI check: bind the **run's exact app bundle path**, inspect its AX tree and screenshot, and require the Whisper window with `History`, `Upload`, and `Dictionary` navigation. An empty History should show `No transcripts yet`. A live process alone is insufficient.

Use `cua_repl` for all UI input. Its first invocation must execute exactly the **`cua_bind` JavaScript command printed by Launch**, which calls `cua.getApp` with the run's exact `bundle` path. Read the returned CUA documentation before further calls. In a later invocation execute the printed `cua_evidence` assignment to set the evidence path. Never select by the display name `Whisper` or the shared bundle ID when the installed app is running. If exact-path binding cannot distinguish the owned process, stop and clean up; do not drive the installed session. After a compacted computer-use session, call `await cua.rewriteDocumentation()` first.

On the validated host, the first AX read exposed only the window/menu. `await app.pressKey("Tab"); await app.getAXState({disableDiffing:true});` activated/populated the GPUI tree. AX clicks could leave the old tree visible until another native event; a subsequent Tab and fresh tree exposed the resulting state. Click the newly observed text field before typing if Tab moved focus. Coordinate clicks returned `Computer Use server error -10005: noWindowsAvailable`; AX-index clicks plus keyboard input worked. Do not keep retrying coordinates after that error.

File isolation permits a panel-only run alongside the installed app, but global shortcuts, menu-bar entries, clipboard, Keychain, OS permissions, and login items are shared. Do not run two agents against the same verification instance. Do not edit API keys: Keychain uses fixed service/account names even under another data directory, and merely opening Settings constructs a Keychain reader. Avoid Settings during the default Dictionary proof. A Keychain authorization prompt is a blocker, not a reason to approve or reset permissions automatically.

On launch failure the helper cleans its own process group and scratch state while retaining `run.json` and any `app.log` already created. Teardown after any later failure is the Cleanup command below.

## Doctor

Run this read-only command first whenever behavior looks wrong:

```sh
uv run --no-project .agents/skills/verify-whisper/scripts/control.py doctor --run-dir "$VERIFY_RUN"
```

It verifies the recorded PID/start identity/process group, executable mapping and SHA-256, data lock, completed startup sweep, SQLite schema, and stored feature state. It never repairs state or supplies authentication. Pair its result with a fresh AX observation: neither a health check nor `Test Connection` proves a transcription journey.

## Drive

Use `app.getAXState()` to obtain current numeric indices. Click controls named by the feature map; GPUI IDs such as `dict-add-word` are source identifiers, **not** DOM selectors or guaranteed AX identifiers. Never carry an index across a UI transition without a new AX observation. If AX does not expose an actionable control, inspect a fresh screenshot, click the visible target, and record that fallback.

For the default offline proof, follow [Dictionary](features/dictionary.md): navigate through `Dictionary`, choose `Add Your First Word`, enter `Verification Quokka`, press Return, and verify the visible row plus a read-only DB query. Navigate to `History` and back through `Dictionary` to confirm the row survives view reconstruction. This proves the empty-state add entry point, not Import, snippets, dictation influence, or every alternate add entry point.

Typical native input calls (indices must come from the latest actual tree):

```javascript
await app.click(index);
await app.getAXState();
await app.typeText("Verification Quokka");
await app.pressKey("Return");
await app.getAXState();
```

Record each action with its intended accessible name and observed outcome in `actions.jsonl` under the run directory. Use real typing/clicks, not setters on app internals, direct DB mutations, or test-only endpoints.

## Evidence

All proof artifacts live in `$VERIFY_RUN` (ignored by git through `/.e2e`), **outside** `scratch`. Save initial, action, result, and reopened AX/screenshot pairs, the action transcript, Doctor output, and stored state:

```sh
uv run --no-project .agents/skills/verify-whisper/scripts/control.py doctor --run-dir "$VERIFY_RUN" > "$VERIFY_RUN/doctor.json"
uv run --no-project .agents/skills/verify-whisper/scripts/control.py state --run-dir "$VERIFY_RUN" > "$VERIFY_RUN/stored-state.json"
```

`state` uses a read-only SQLite connection and reports Dictionary/snippet contents plus History/analytics counts. Run it before cleanup. It is a second observation, not a way to seed or drive the feature.

In CUA, standard Node filesystem writes can persist observations without synthesizing UI input:

```javascript
var fs = await import("node:fs/promises");
await fs.writeFile(evidence + "/before.ax.txt", await app.getAXState({emit:false, disableDiffing:true}));
await fs.writeFile(evidence + "/before.png", await app.getScreenshot({emit:false}));
```

Set `evidence` using the Launch output as described above. Repeat as `action`, `after`, and `reopened`. A full AX snapshot can renumber indices, even with `emit:false`; after saving screenshots or hidden AX captures, emit a fresh full tree before choosing an index. Capture the command/action and resulting state, not only a final screen. Mutation proofs require stored state alongside visible feedback; upload/dictation proofs additionally require resulting transcript/audio/files and the intended pasted text or clipboard. Use only disposable content. Do not log credentials or capture API-key inputs. Mocks are permitted only at existing production boundaries (for example an ASR HTTP server), must be disclosed, and cannot establish real-server or microphone compatibility.

There is no dry-run flag. The panel workflow suppresses hotkeys but still initializes the native app, a lid watcher, menu bar, retention cleanup, and scratch files. Do not claim it is globally side-effect-free or network-isolated. Record the actual scene, empty initial data, processes, files, and configured endpoints; offline Dictionary success proves no ASR behavior. Unexpected network or authorization activity must be investigated, not dismissed because this is a verification run.

For UI code changes, capture each changed view before and after, embed the pairs in the reply, and identify views that render identically. Motion/timing changes also need a short video using a recording facility available in that session; static snapshots cannot prove timing. Creating this skill does not change app UI.

## Cleanup

```sh
uv run --no-project .agents/skills/verify-whisper/scripts/control.py cleanup --run-dir "$VERIFY_RUN"
ls -l "$VERIFY_RUN"/before.png "$VERIFY_RUN"/after.png "$VERIFY_RUN"/stored-state.json "$VERIFY_RUN"/cleanup.json
```

The helper checks ownership, terminates only its recorded process group (including its native helpers), escalates after five seconds if needed, and removes `scratch`. It never kills by name or touches the installed app. Run it after **every** failed attempt too. `Cmd+W` only hides/closes the control panel and leaves the menu-bar process running, so it is not teardown. Retain evidence, logs, `run.json`, and `cleanup.json`; confirm the screenshot/state files still exist after cleanup. Finish the CUA portion with exactly one `completion_summary` call after observing its final outcome.

## Helpers

[`scripts/control.py`](scripts/control.py) is executable. The commands above show all four actions: `launch`, read-only `doctor`, read-only `state`, and `cleanup`; every action requires `--run-dir`. Do not reuse a cleaned run for another launch. Rebuild and launch a new run after changing the executable.

When an entry point, accessible name, or stored result changes, update the corresponding recipe in the [feature map](features/README.md).

## Validated baseline

The initial run on macOS built the actual checkout with `cargo build --locked`, launched from empty data, passed Doctor, and drove `words-add` through `Add Your First Word` with real AX clicks/typing/Return. `Verification Quokka` appeared on screen, survived History-to-Dictionary navigation, and existed as one `manual` SQLite row with zero uses; History and analytics counts remained zero. Cleanup terminated the owned app/helper processes and removed scratch while the installed app continued running. Screenshots, AX snapshots, actions, Doctor, stored state, and cleanup evidence survived under `.e2e/verify-whisper/proof-dictionary-bundle/`. Additional helper checks covered failure before spawning, metadata-write failure after spawning, and repeat cleanup; their results are in `helper-failure-checks.json` beside the UI evidence.

This ran on the actual local Mac, not a container or simulated desktop. The temporary wrapper is scaffolding for CUA; this proof did not establish packaged-release behavior, process-restart persistence, real microphone/server transcription, or the other entry points in the feature map.
