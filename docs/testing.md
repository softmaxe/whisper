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

## Dropdown interaction regression check

- In Settings, open and close each select by clicking its trigger again: interface language, speech language, Chinese script, microphone, recording pill position, audio retention, and transcript retention. Repeat with a slow press and release and with several consecutive clicks.
- Check that selecting an option closes the menu and applies the value, while clicking elsewhere closes it without changing the value. Switching directly between selects should leave only the new menu open.
- In the searchable speech language menu, type a query and select a result. Reopen it and check that the query resets. Check arrow keys, Return, Escape, and Tab as well.
- Repeat trigger toggling, slow clicks, outside dismissal, and keyboard activation for a History row's actions, the History page actions, and the Dictionary page actions. Check that Dictionary context menus still dismiss on outside clicks.

## Settings scrolling regression check

- With a populated History page, open Settings and repeatedly scroll Text cleanup with a mouse wheel, then switch sections and click controls. Repeat while the server connection check is pending and after it fails or succeeds.
- Test the prompt preview, Customize and Test tabs, scrolling to both ends of the page. Controls should respond promptly after rapid scrolling.
- In Prompt Studio, scroll a long prompt with the pointer inside View and Customize. Only the prompt viewport should move, even at its top and bottom; scrolling outside it should still move Settings. Repeat with a mouse wheel and a trackpad.
- Save a custom prompt containing headings, bold text, lists, inline code, and a fenced code block. View should render Markdown with wrapped prose; Customize and Copy should preserve the Markdown source. Verify the preview updates after saving or resetting and with both interface languages and themes.
- Close Settings and verify the History scroll position is preserved. Repeat from Insights, Upload and Dictionary.
- Repeat in a narrow window where Settings covers the workspace and a wide window where it floats above it. Check opening and closing animations, Escape, keyboard focus, and window resizing with Settings open.
- When profiling the full-window sheet after its opening animation, the covered workspace must not participate in layout or paint. The floating sheet and closing animation still render the visible workspace.

## Release smoke check

Automated checks do not establish microphone, Accessibility, or server compatibility on a user's Mac. Before calling a release ready, check the packaged app on macOS:

- Configure the intended ASR and cleanup servers, dictate with the global shortcut, and verify the pasted result and cleanup failure fallback.
- Switch microphones, exercise Auto selection with the laptop lid, and verify recording cancellation.
- Retry a History item, then check copy, delete, and retention settings.
- Upload a file and a batch with History both enabled and disabled. Check raw results remain copyable and do not increase dictation Insights.
- Verify dictionary and snippet behavior, the menu bar toggle, and installation through the Homebrew cask.

For changes to macOS signing, also test an upgrade between two release builds signed with the same saved certificate:

1. Build version A with `scripts/bundle-macos.sh --release`. Record its designated requirement with `codesign -d -r- dist/Whisper.app` and verify it with `scripts/sign-macos-release.sh verify dist/Whisper.app`.
2. Install version A at `/Applications/Whisper.app`. Grant microphone and Accessibility access, save a test server credential, and permit Keychain access when prompted. Dictate and verify automatic paste.
3. Build version B with a higher app version using the same certificate and private key. Repeat the signature checks and compare its designated requirement with version A. The requirement must stay the same even though the version and code hash differ. Check the helper requirements as well.
4. Quit version A and upgrade to version B through the Homebrew cask, preserving the installation path. Launch it, dictate, verify automatic paste, and access the saved credential. Record whether microphone, Accessibility, or Keychain authorization appears again and the macOS version used.

Do not reset TCC permissions, change system trust, or delete Keychain items during the upgrade test. Those actions would invalidate the test. Automated signature checks alone do not prove permission retention, and a source change does not update an already installed app. See [macOS signing](macos-signing.md) for identity setup.
