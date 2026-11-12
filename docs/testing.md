# Testing

Tests follow the supported features in the root [README](../README.md). Remove tests for retired features instead of hiding them with `#[ignore]`.

## Run

```sh
scripts/check.sh
```

The script runs `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings`, and `cargo test --locked`. It is the same command the CI workflow runs. Use the Rust version pinned in [`rust-toolchain.toml`](../rust-toolchain.toml); rustup selects it automatically.

Unit tests live next to the code in `src/`. Tests marked `#[ignore]` need hardware or a live server; run them by name with `cargo test -- --ignored <name>`.

## Workflows

The CI workflow lints the workflows and scripts and runs `scripts/check.sh` on pull requests and pushes to `main`. Pull requests that only change documentation skip the Rust checks. The `CI result` job is the only required status check; it passes when every other CI job passed or was skipped.

Pull requests also build an ad-hoc signed package when they change the Build workflow, `build.rs`, `packaging/`, `resources/`, packaging scripts, or the application icon inputs (`assets/icon.icns`, `assets/Assets.car`, and `assets/whisper.icon/`). Changes limited to embedded UI icons, fonts, ordinary Rust code, or CI orchestration run the regular checks without building a package. Run the Build workflow manually when a package is needed for other changes.

Both pull requests and pushes to `main` can restore Cargo output from an older lockfile with the same runner, architecture, toolchain, Cargo configuration, and CI build settings. Cargo rebuilds dependencies affected by the lockfile change. Only successful checks on `main` save new caches. The cache namespace is versioned: changing it causes one cold build on `main` before compatible runs can reuse the new cache, and also provides a way to discard accumulated obsolete artifacts.

The Release workflow builds and publishes a signed package for a `v<version>` tag on `main` once CI has passed on that commit. See the workflow files for the exact steps and [macOS signing](macos-signing.md) for the release identity.

## Sidebar interaction regression check

- Check that the native window buttons, sidebar toggle, brand, and page title share the centerline of the compact 40-point top bar. The toggle's hit target must stay 18 points clear of the green window button, with 12 points between the toggle's hit target and the brand. In the collapsed state, the page title should start at the same inset as the expanded brand. Repeat with Settings covering the window and after leaving fullscreen.
- Leave the pointer on the sidebar toggle and repeatedly click it, including slow press/release and reversals before the animation finishes. The toggle must stay beside the native window controls, with no unexpected reopening or window movement. Hovering it or the left edge must never change the sidebar state.
- Verify the sidebar and workspace move together without an overlay, clipping leaks, or a jump at either end. Check Home, Insights, Upload, and Dictionary with a long transcript list and at the minimum window size. The active page and scroll position must survive toggling.
- Check that the brand slides with navigation within its own clipping slot, while the window controls and sidebar toggle stay fixed. During the 200 ms transition and reversals, no brand text may cross into the toggle or window controls.
- Watch the toggle itself: its chevron must morph into the divider on collapse and emerge from it on expansion, with the outline stationary. Click again during the morph; the path must continue from its visible shape. With Reduce motion enabled, the icon must snap directly to its final shape.
- Activate the toggle with Tab and Return or Space. After collapse completes, Tab and Shift-Tab must skip hidden navigation; after expanding, navigation must be reachable again. Repeat after closing Settings and command search.
- Reopen the app in each sidebar state and confirm the saved state appears immediately. Check both interface languages and light, dark, and Tokyo Night themes. With macOS Reduce motion enabled, toggling must change layout immediately.

## Dropdown interaction regression check

- Check that every Settings select menu shares both horizontal edges with its trigger, including searchable language and model menus and the variable-width microphone picker. Repeat after changing the microphone name and resizing the window with a menu open; long options should truncate inside the menu without widening it.
- In Settings, open and close each select by clicking its trigger again: interface language, output mode, speech language, Chinese script, microphone, recording pill position, audio retention, and transcript retention. Repeat with a slow press and release and with several consecutive clicks.
- Check that selecting an option closes the menu and applies the value, while clicking elsewhere closes it without changing the value. Switching directly between selects should leave only the new menu open.
- In the searchable speech language menu, type a query and select a result. Reopen it and check that the query resets. Check arrow keys, Return, Escape, and Tab as well.
- Repeat trigger toggling, slow clicks, outside dismissal, and keyboard activation for a History row's actions, the History page actions, and the Dictionary page actions. Check that Dictionary context menus still dismiss on outside clicks.

## Cleanup model picker regression check

- Open Settings > Text cleanup with a configured server. The Model field should show only the selected ID, with connection status and Test Connection on one compact row below it.
- Open the model picker, search for a listed ID, and select it using both the pointer and arrow keys plus Return. Verify the menu closes and the chosen ID persists after reopening Settings.
- Enter an unlisted model ID, including a mixed-case name with a source prefix and tag. Choose the Use action, then verify the saved ID preserves case and trims surrounding whitespace. Repeat before any server models have loaded and after a connection failure.
- Reopen the picker and verify the search resets. Close it using Escape, Tab, a second trigger click, and an outside click; none should change the model. Check long IDs in a narrow window and verify the full value is available in a tooltip.
- Change the server URL and verify choices from the previous server disappear while editing. Test Connection should load the new server's models without replacing a manually selected ID. Disable cleanup and verify the picker is unavailable.
- With a server exposing models only under `/v1`, enter its address without `/v1` and test both speech-to-text and cleanup. The visible and saved addresses should adopt `/v1`; cleanup models and connected status should remain available, and real dictation should use the discovered speech endpoint. Delay the response and type a different address before it arrives; the old result must not replace the new draft or saved address.
- Repeat with both interface languages and themes. Confirm the existing language and microphone dropdown interactions still work.

## API key editing regression check

For the Settings API key editor, change a test credential and cancel with the mouse, Tab then Return, and Escape. Reopen the editor and verify the original credential remains. Tab between the input, Save, and Cancel without saving; moving focus outside the entire editor should save once. Verify Save and Enter still save, including an empty value to clear the key. Use only disposable test credentials and never record their values in screenshots or logs.

## Local data editing regression check

- Add several Dictionary words in quick succession, including case variants and pasted lists. Rename a word, delete it, then add it again before starting another action. Verify duplicate feedback, row order, source, and keyboard focus remain consistent.
- Delete words, corrections, and snippets with both the mouse and keyboard. Undo within the existing undo window, then repeat and let the deletion commit. Clear the Dictionary while a deletion is pending and verify no row reappears.
- Create and edit several snippets quickly. Check duplicate triggers, case-only renames, insertion order, and empty or overlong triggers. Close the editor immediately after saving and verify the result survives reopening and app restart.
- Keep Settings open while dictating, uploading, and editing local data. Verify its selected values remain stable, while recording status and microphone information still update. Change the interface language and retention settings and verify option labels and enabled states update.
- Upload files and retry History rows with the Dictionary visible before and afterward. Dictionary usage must remain unchanged. Dictation must still update usage with History retention disabled.

Storage failure and callback-order regressions use temporary databases in automated tests. Pipeline regressions also verify that failed edits never become transcription hints or replacements, and that edits after a request starts do not change its captured rules. Do not damage the installed application's database to simulate a write failure.

## History deletion regression check

- Delete a History row, then close the panel within six seconds using the native close button. Reopen it and restart the app; the row must stay deleted. Repeat with the panel's keyboard close command.
- Delete a row, Undo, then delete the same row again before its first six-second deadline. It must remain undoable until the second deletion's deadline.
- Hover the Undo toast, or focus its action, for more than six seconds. The toast must disappear at the original deadline and the deletion must commit. Ordinary notification toasts should still pause on hover.
- Delete A, then B while A is undoable. A must commit and its toast must disappear immediately; B must have its own full undo window. Undo B and verify only B returns.

## Settings scrolling regression check

- With MOS enabled, reverse the wheel immediately after reaching either end of Settings and the prompt preview. The first reverse event should move content immediately, without repaying overscroll. Repeat with MOS disabled and with several small, fractional pixel events between frames.
- With a populated History page, open Settings and repeatedly scroll Text cleanup with a mouse wheel, then switch sections and click controls. Repeat while the server connection check is pending and after it fails or succeeds.
- Test the prompt preview, Customize and Test tabs, scrolling to both ends of the page. Controls should respond promptly after rapid scrolling.
- In Prompt Studio, scroll a long prompt with the pointer inside View and Customize. Only the prompt viewport should move while there is content in that direction; continuing past its top or bottom should scroll Settings. In View, a wheel delta crossing an edge should pass only its unused distance to Settings. Repeat with a mouse wheel and a trackpad, including rapid direction changes and several events between frames.
- With the pointer outside the prompt, scrolling should move only Settings, even as the prompt viewport passes the top of the page. The prompt's internal scroll position should remain unchanged. Repeat in both directions with a large wheel delta, a short prompt, and a resized window.
- In View, repeat with hundreds of Markdown paragraphs and list items, rapidly scrolling in both directions. The preview should keep a fixed height and render only visible blocks; controls should remain responsive. Check a short prompt as well, and verify text selection and Copy still work after scrolling and resizing.
- Save a custom prompt containing headings, bold text, lists, inline code, and a fenced code block. View should render Markdown with wrapped prose; Customize and Copy should preserve the Markdown source. Verify the preview updates after saving or resetting and with both interface languages and themes.
- With the default prompt, switch the interface language in both directions and compare View, Customize, and Test. Saving the translated default should keep the prompt in default mode. Repeat with an unsaved edited draft and a saved custom prompt; neither should be overwritten. Check the speech-language Auto-detect label, search placeholders, and cleanup model search after switching language.
- Close Settings and verify the History scroll position is preserved. Repeat from Insights, Upload and Dictionary.
- Focus Dictionary search, open Settings with the keyboard, then close it with Escape, the close button, and the outside backdrop. After the exit animation, typing should return to Dictionary search. Repeat at narrow and wide window sizes.
- Repeat in a narrow window where Settings covers the workspace and a wide window where it floats above it. Check opening and closing animations, Escape, keyboard focus, and window resizing with Settings open.
- When profiling the full-window sheet after its opening animation, the covered workspace must not participate in layout or paint. The floating sheet and closing animation still render the visible workspace.

## Permission refresh regression check

For Settings permission rows, use a test Mac or test account with undetermined microphone access. Accept or deny the first prompt and verify the row updates without switching sections. Change microphone or Accessibility access in System Settings, return to Whisper, and verify both rows refresh. Close Settings while the first prompt is pending and verify no late result affects a reopened sheet. Do not reset permissions on the installed release for this check.

## Release smoke check

Automated checks do not establish microphone, Accessibility, or server compatibility on a user's Mac. Before calling a release ready, check the packaged app on macOS:

- Configure the intended ASR and cleanup servers, dictate with the global shortcut, and verify the pasted result and cleanup failure fallback.
- Switch microphones, exercise Auto selection with the laptop lid, and verify recording cancellation.
- Keep Settings open on the microphone picker with the system default set to the MacBook input. In Auto, close the lid and verify that "Next recording uses" updates to the available iPhone without pressing Refresh; reopen the lid and verify the built-in input returns. Connect and disconnect an external input and check that the options update without dismissing an open picker.
- Press Refresh repeatedly, then close Settings while a refresh is pending. Reopen Settings and verify fresh devices appear; a closed sheet must not keep polling or publish a late result.
- Click the microphone Refresh button with the mouse and keyboard. Its icon should turn once over 600 ms and ease to a stop without shifting the button or picker. Rapid clicks should keep the current turn smooth while still requesting refreshes. With macOS Reduce motion enabled, the icon should stay still and the device list should still refresh.
- Start dictation and verify "Currently using" reports the device that opened. Change the picker or lid during recording: the current device must remain unchanged, with the new selection taking effect on the next recording. Stop or cancel and verify the row returns to "Next recording uses". Check unavailable saved inputs and default-input changes too.
- Retry a History item, then check copy, delete, and retention settings.
- Upload a file and a batch with History both enabled and disabled. Check raw results remain copyable and do not increase dictation Insights.
- Verify dictionary and snippet behavior, the menu bar toggle, and installation through the Homebrew cask.

For changes to macOS signing, also test an upgrade between two release builds signed with the same saved certificate:

1. Build version A with `scripts/bundle-macos.sh --release`. Record its designated requirement with `codesign -d -r- dist/Whisper.app` and verify it with `scripts/sign-macos-release.sh verify dist/Whisper.app`.
2. Install version A at `/Applications/Whisper.app`. Grant microphone and Accessibility access, save a test server credential, and permit Keychain access when prompted. Dictate and verify automatic paste.
3. Build version B with a higher app version using the same certificate and private key. Repeat the signature checks and compare its designated requirement with version A. The requirement must stay the same even though the version and code hash differ. Check the helper requirements as well.
4. Quit version A and upgrade to version B through the Homebrew cask, preserving the installation path. Launch it, dictate, verify automatic paste, and access the saved credential. Record whether microphone, Accessibility, or Keychain authorization appears again and the macOS version used.

Do not reset TCC permissions, change system trust, or delete Keychain items during the upgrade test. Those actions would invalidate the test. Automated signature checks alone do not prove permission retention, and a source change does not update an already installed app. See [macOS signing](macos-signing.md) for identity setup.

## Insights regression check

- Using a temporary development database with at least 3,000 active days, select All time. Check that the date range starts at the earliest activity, active days include the full history, and chart totals, spoken time, and busiest day agree with the headline totals. Hover, keyboard navigation, and range switching should remain responsive; long charts should use weekly bars.
- Leave the app open across local midnight without new dictation. Revisit Insights from another page and reactivate the window from another app. The streak number, calendar, range dates, and chart should refresh together. Repeat with a streak whose last activity was yesterday before midnight, so it expires afterward. A failed refresh should show Retry without repeatedly loading on every render.

## Tokyo Night theme check

- Select Tokyo Night in Settings > General. Reopen Settings and restart the app; the selection must persist. Switch macOS appearance while it is selected: it must remain Tokyo Night. Verify Auto still follows macOS, and Light and Dark remain available.
- Review Home with transcripts and an empty history, Insights with both current and comparison data, Upload, Dictionary words and snippets, command search, and each Settings section. Use both interface languages and the minimum supported window size. Check reading text, metadata, chart labels, selected navigation, popovers, and input edges.
- Use `WHISPER_DEV_SCENE=settings:language` to inspect the expanded interface-language menu in a debug build.
- Tab through theme choices, buttons, switches, inputs, and dropdowns. Check focus, hover, selected, disabled, and destructive states. The filled action must retain a clearly contrasting label; switches must remain distinguishable by both thumb position and track color.
- With `WHISPER_USER_DATA_DIR` pointing to a temporary directory, use `WHISPER_DEV_SCENE` in a debug build to inspect `pill:recording`, `pill:processing`, `pill:preparing`, `pill:error`, and `pill:fallback`. Tokyo Night uses the same indigo surface family as the workspace, muted blue voice activity, and distinct error/recovery colors. Light and Dark retain their dark floating HUD.
- Automated tests cover preference serialization, system-appearance resolution, reading contrast, focus indicators, and selected/action text. They complement visual review; they do not prove native interaction or rendering quality.

## Filler-word removal regression check

- In Settings > Speech to text, toggle Remove filler words, restart, and verify the preference persists. Existing settings enable it by default.
- With cleanup off and English selected, dictate “Um, we should go”; delivery should read “We should go” while History's raw transcript retains the hesitation. With Auto selected, “um carro” must remain intact.
- Check Chinese hesitation runs and the exceptions 打呃 and 呃逆. Newlines, ordinary words, and the non-filler Chinese words 嗯, 啊, 额, 那个, 然后, and 就是 must remain.
- Check adjacent fillers leave one space between surviving English words, preserve newlines, and remove filler-owned punctuation including ellipses. `foo uh,uh,bar` should become `foo bar`, `hello. Um... we go` should become `hello. We go`, and `好的，呃。我们走` should become `好的。我们走`.
- Check URLs, emails, backtick-wrapped tokens, member access, attached code punctuation and assignments remain intact (`https://uh.edu/a`, `user@hm.com`, `uh_count`, `uh-oh`, `let uh = 1;`). A spaced hesitation dash (`um - I think`) must still filter in Auto.
- Check filler-only input, including `uh!` and `呃。`, produces no-audio feedback. Punctuation-only server text with no removed filler keeps its existing delivery behavior. Whole snippets must match the raw transcript first (`uh oh` and even the filler-only trigger `hmm`), then the filtered transcript (`Um, my address` for `my address`). Turn removal off and verify the transcript is unchanged. File uploads must continue returning raw server text.
- Tab to the switch and activate it with the keyboard; verify its VoiceOver label in both interface languages and all three themes.

## Skip cleanup regression check

- Delay the cleanup server, dictate, and verify Transcribing becomes Polishing only after ASR completes. Skip must appear inside the capsule while Cancel remains beside it. Skip is unavailable during ASR.
- Click Skip and verify immediate insertion of the recognized text with filler removal, Chinese script conversion, known corrections, and snippets. There must be no cleanup failure notice and no second paste when the delayed reply arrives. Cancel must still discard the whole operation.
- Tab to Skip and activate it with Return or Space. Check VoiceOver's “Skip cleanup” label and the “Insert without cleanup” hover hint. Check smooth width changes and emergence, then repeat with Reduce motion for immediate changes.
- Use `WHISPER_DEV_SCENE=pill:cleanup` with isolated data to inspect Light, Dark and Tokyo Night, and both interface languages.
- Use `WHISPER_DEV_SCENE=pill:flow` to watch one whole Dictation on a loop: recording, transcribing, polishing, Skip leaving, and the delivered check. The meter and the label must never overlap, and the pill must not read "Transcribing" again after it has read "Polishing".

## Microphone loopback regression check

- In Settings > Dictation, check the microphone control order: Test, Refresh, device picker. Test must use the same resolved device as the next dictation, including Auto, lid changes, unavailable saved devices, and the system-default fallback.
- Test retains at most ten seconds with a live level meter, countdown, and Stop. Even if UI polling is delayed beyond the deadline, manual stop and auto stop must truncate the retained samples to ten seconds at the capture sample rate. Stop early, play, stop playback, replay, and Record again. The progress bar, duration and silence warning should reflect only the retained memory clip. A silent clip remains playable and shows a warning.
- Deny microphone permission, disconnect the selected device, and test a device delivering no frames. Check the named error and Try again. Disconnect output during playback and check its error as well.
- Start dictation with the hotkey during recording and playback. The test must close and yield audio before dictation begins. While dictation records, prepares, or processes, Test is disabled with an explanation. No test may play dictation cues or pause media.
- Close Settings, change section, change device, and drop/reopen the sheet during microphone startup, recording and playback. Each must release the streams and clip, and no old result may reopen the strip. No test audio may appear in History, recovery, storage or requests.
- Check Tab, Return/Space, VoiceOver names, Light/Dark/Tokyo Night, both languages, and Reduce motion. Use `settings:mic-recording`, `settings:mic-ready` and `settings:mic-error` dev scenes with isolated data for visual checks.
- `cargo test -- --ignored live_microphone_loopback` checks real capture and output with microphone authorization; ordinary lifecycle tests use an injected clock and hardware-free audio.

## Local silence filtering regression check

- In Settings > Speech to text, verify Skip silence defaults on, persists, and is named for VoiceOver. Check both languages, all themes, and keyboard activation.
- Record speech with pauses and background noise. Only the uploaded copy should be trimmed, preserving a 480 ms lookback and tail. Confirm adopted pre-roll is filtered too. History, recovery, recording duration and Insights words per minute must use the original recording.
- Try silence and broadband noise: VAD should avoid a server request and show the existing no-audio wording with a kept recording. Send anyway must send the original once, while Cancel and Record New still discard it. Turn filtering off and verify upload bytes remain untrimmed. File uploads remain outside this path.
- Retry History audio and verify filtering applies with current settings while the stored audio and duration remain unchanged. When speech is detected, send the trimmed copy; when no speech is detected, send the original stored WAV because retry explicitly requests that recording. Live dictation must still avoid a request and offer Send anyway. Check model/inference failures fail open and a short recording is sent intact.
- Automated tests exercise production frame retention with scripted probabilities, detector failures, partial final frames, a real speech fixture in silence and broadband noise, and independent ONNX Runtime reference probabilities. `cargo test --release sixty_second_recording_timing -- --nocapture` logs the 60-second latency without a brittle CI timing limit.

### VAD model spike and fixture provenance

`tract-onnx` 0.23.10 was checked against its current crate API and tested before integration. The general v6.2.2 op15 model fails static typing at a decoder `If` (branch ranks differ), while the official fixed-shape 16 kHz OpenVINO ONNX export loads directly. Both official exports give identical reference probabilities on the fixture. Anonymous in-memory node names are made unique before tract optimization so debug validation and release inference use the same graph; weights and connections remain unchanged. Tract identifies 83 of its 90 frames as speech at >0.3; three seconds of silence and seeded broadband noise identify zero speech frames. A separately evaluated `silero-vad-pure` 0.1.1 produced incorrect low speech probabilities and is not a dependency.

The small `tests/fixtures/vad-speech.wav` was generated locally with macOS `say -v Samantha` saying “Please send the design review to Thursday afternoon.” and converted with `afconvert -f WAVE -d LEI16@16000 -c 1`. It is 16 kHz mono PCM16, 93 KB, and contains no user recording. `vad-probabilities.json` records the official model's CPU ONNX Runtime probabilities for consecutive 512-sample frames, using zero initial `[2,1,128]` state, 64 samples of left context, and zero padding on the final frame. The production tract test compares every probability within 0.00002.

On the implementation machine (Apple M2 Pro, macOS 27.2.1), the isolated release timing test processed 60 seconds in **116.397 ms**, including first model initialization. The release executable grew from **44,016,400 bytes** before VAD to **67,235,808 bytes** with VAD (+23,219,408 bytes, about 22.14 MiB). These are executable sizes, not app bundles. `cargo build --release --locked --offline` passed; `otool -L` listed only Apple/system dependencies, with no ONNX Runtime or other new distributed dylib. No release packaging or signing was run.
