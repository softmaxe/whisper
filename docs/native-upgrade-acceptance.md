# Native signed-upgrade acceptance

This procedure has not been executed against complete accepted native builds.
Automated signature checks are recorded separately in the
[migration checkpoint](native-migration-status.md). Execute the installation and
physical checks with the user present, after full feature/performance acceptance
and signed-artifact readiness. Publication and public tap updates require a
separate request.

## Findings and recommended path

Use one local-only tap with one distinct cask token, `whisper-native-acceptance`,
whose artifact remains **Whisper.app at /Applications/Whisper.app**. Install the
complete signed native A, approve its initial permissions and save fresh native
credentials, change that same local cask definition to higher-version B, then run
an actual targeted `brew upgrade --cask`. No GitHub release is needed. Homebrew
supports local taps and file URLs; its own upgrade tests use local ZIPs. Sources:
[local taps](https://docs.brew.sh/How-to-Create-and-Maintain-a-Tap),
[file-URL upgrade test](https://github.com/Homebrew/brew/blob/2f1c682db046d37c4b6c09aa43837be6ff270c39/Library/Homebrew/test/cask/upgrade_spec.rb#L748).

The canonical path cannot hold legacy and native simultaneously. Tomorrow's
controlled replacement window must first preserve the legacy bundle, its cask
receipt/definition and recovery archive, then normally uninstall the legacy cask
without `--zap`. A distinct acceptance token avoids changing the production tap
or keeping two casks claiming the same application. A nonstandard `--appdir` may
be useful for a preliminary packaging check, but it does not satisfy this repo's
canonical-install-path smoke check. The repo contract is in
`test/README.md#release-smoke-check` and `docs/macos-signing.md`.

Observed locally on 2026-09-20:

- Homebrew `7.0.4-17-g2f1c682`, source HEAD
  `2f1c682db046d37c4b6c09aa43837be6ff270c39`, prefix `/opt/homebrew`.
- Installed `softmaxe/tap/whisper` is 1.0.5 and uses `/Applications`.
  Its Caskroom application is a symlink to `/Applications/Whisper.app`.
- Installed bundle ID is `local.whisper.desktop`. Its designated requirement is
  already bound to certificate leaf SHA-1
  `13a893e466c3e18cffd9351fa4f0b3bf075c27af`, matching the public PEM fingerprint.
  This is not currently an ad-hoc main-app signature. That does not prove all
  permission behavior, or replace static verification of every helper during that session.
- The existing cask has `app "Whisper.app"` and a separate `zap` listing the legacy
  profile, cache and preferences. No `--zap` may be used in this protocol.
- Native configuration defaults to `~/Library/Application Support/WhisperNative`,
  with Keychain service `local.whisper.desktop.native`; legacy data remains under
  its existing `whisper` profile. Native `--profile` selects an isolated directory
  without changing bundle identity or that native Keychain service.
- `scripts/package-native.js` supports `--release` and `--development`, reads a
  numeric X.Y.Z from package.json, and generates bundle/version-matched ZIP and
  checksum. It has no version override flag. Two disposable worktrees with
  temporary package.json/package-lock.json version edits are sufficient; no
  product change is needed for this plan.
- Homebrew 7 compact Cask metadata JSON can legitimately be `{}`; the local
  INSTALL_RECEIPT contains source version and uninstall artifacts. Do not treat
  that JSON alone as corrupted or edit receipts to force an upgrade. See
  [installed metadata source](https://github.com/Homebrew/brew/blob/2f1c682db046d37c4b6c09aa43837be6ff270c39/Library/Homebrew/cask/cask.rb#L701).

The preparatory inspection did not read credential file contents or Keychain
items. Recheck the installed version and cask definition before executing this
procedure.

## Prerequisites for the attended session

1. Both complete native builds must have accepted source revisions and documented
   full-feature/performance results. Resolve all current integration/review issues
   first. Both must use the original certificate/private key and unchanged main
   and helper IDs. Missing/mismatched signing credentials stop the test; never run
   signing:create, use ad-hoc fallback, or edit a signed Info.plist afterward.
2. Reserve two numeric versions A < B, for example 1.0.6 and 1.0.7 only if those
   are appropriate at execution time. Do not tag, commit these temporary version
   edits, or publish them. Prefer two accepted source revisions with an actual
   code change. If one accepted SHA is built twice with only different version
   metadata, record that limitation explicitly; the actual Homebrew version
   upgrade still runs, while changed-executable/helper continuity has separate
   automated test:signing evidence.
3. Have the original private signing backup available through the established
   local setup, enough disk space for both builds and a verified legacy backup,
   and working test ASR/cleanup services. For credential proof, at least one
   endpoint should require the saved synthetic acceptance credential, or otherwise
   provide a boolean authentication result. Never put real credentials in notes,
   shell history, command arguments, screenshots, server logs or the repository.
4. Reserve an attended window with no other Homebrew operations. Keep the same
   Homebrew version during A→B; automatic update/cleanup is disabled only in the
   command environment below. Do not reset TCC, delete Keychain items, change
   certificate/system trust, disable Gatekeeper, or strip quarantine attributes.
5. Prepare an editable disposable target and the built-in plus wireless iPhone
   microphone categories confirmed by the user. Record categories, not private
   device labels. Use synthetic spoken text. Production server compatibility and
   performance budgets remain separate acceptance evidence.

## 1. Build and preserve artifacts, before changing /Applications

Run from the repository. All snippets below are future commands. Fill the accepted SHA/version values;
use one private, durable acceptance directory. No root checkout edits are needed.

```sh
set -euo pipefail
WHISPER_REPO="$(git rev-parse --show-toplevel)"
WHISPER_ACCEPT="$HOME/Library/Application Support/Whisper Upgrade Acceptance/REPLACE_RUN_ID"
WHISPER_A_SHA='REPLACE_WITH_ACCEPTED_NATIVE_SHA_A'
WHISPER_B_SHA='REPLACE_WITH_ACCEPTED_NATIVE_SHA_B'
WHISPER_A_VERSION='1.0.6'
WHISPER_B_VERSION='1.0.7'
WHISPER_A_TREE="$WHISPER_ACCEPT/build-A"
WHISPER_B_TREE="$WHISPER_ACCEPT/build-B"
mkdir -p "$WHISPER_ACCEPT/artifacts" "$WHISPER_ACCEPT/evidence" "$WHISPER_ACCEPT/legacy"
chmod 700 "$WHISPER_ACCEPT"
export PATH="/opt/homebrew/opt/node@24/bin:$PATH"
export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
export HOMEBREW_NO_AUTO_UPDATE=1 HOMEBREW_NO_INSTALL_CLEANUP=1 HOMEBREW_NO_ANALYTICS=1

git -C "$WHISPER_REPO" worktree add --detach "$WHISPER_A_TREE" "$WHISPER_A_SHA"
git -C "$WHISPER_REPO" worktree add --detach "$WHISPER_B_TREE" "$WHISPER_B_SHA"
```

For each tree, set its version before checks and signing:

```sh
cd "$WHISPER_A_TREE"
npm version "$WHISPER_A_VERSION" --no-git-tag-version --ignore-scripts
npm ci
npm run quality-check
node scripts/build-native-ffmpeg.js
npm run native:test
npm run test:signing
npm run pack:release
cp "release/whisper-$WHISPER_A_VERSION-macos-arm64.zip"* "$WHISPER_ACCEPT/artifacts/"
```

Repeat with B tree/version. Both archives and checksum files must remain separate.
Run `shasum -a 256 -c` in the artifacts directory for each .sha256 file. Do not
modify either signed app or archive afterward. Existing package verification
checks app/version/minimum OS/arm64/runtime dependencies, pinned signatures, and
the extracted archive; it does not install anything.

Record the actual main and helper requirements for A and B, using each build's
`packageCode(app)` list from `scripts/lib/native-packaging.js`. Compare matching
relative paths and require each actual target in B to satisfy A's saved
requirement, and vice versa. At minimum record:

```sh
codesign --verify --deep --strict "$WHISPER_A_TREE/dist/native-arm64/Whisper.app"
codesign -d -r- "$WHISPER_A_TREE/dist/native-arm64/Whisper.app" 2>&1
codesign --verify --deep --strict "$WHISPER_B_TREE/dist/native-arm64/Whisper.app"
codesign -d -r- "$WHISPER_B_TREE/dist/native-arm64/Whisper.app" 2>&1
```

Use the existing `verifySignature(app, packageCode(app))` functions to verify the
pinned requirement for every packaged target, not just the main bundle. Keep
public requirement/CDHash/version/checksum evidence; do not capture private-key
or signing-password output. A/B CDHashes and versions should establish which two
builds were tested. If a helper inventory differs, record new helpers separately.

## 2. Preserve the legacy recovery path

Quit the installed legacy app normally, allowing its writes to finish. Verify no
Whisper app/helper process remains; use PID-only process checks rather than full
argument/environment dumps. Do not kill an active recording to make the test fit.

```sh
codesign --verify --deep --strict /Applications/Whisper.app
/usr/bin/ditto -c -k --sequesterRsrc --keepParent /Applications/Whisper.app \
  "$WHISPER_ACCEPT/legacy/Whisper-installed-backup.zip"
cp "$(brew --repository softmaxe/tap)/Casks/whisper.rb" "$WHISPER_ACCEPT/legacy/whisper.rb"
/usr/bin/ditto "$(brew --caskroom whisper)/.metadata" "$WHISPER_ACCEPT/legacy/cask-metadata"
shasum -a 256 "$WHISPER_ACCEPT/legacy/Whisper-installed-backup.zip" \
  > "$WHISPER_ACCEPT/legacy/Whisper-installed-backup.zip.sha256"
```

Verify the backup ZIP by extracting it to a private verification directory and
running codesign verification and comparing its main/helper requirements/CDHashes
with the installed original. Remove that extracted verification copy afterward,
leaving the ZIP for recovery; do not launch a duplicate app with the same ID.
Optionally prefetch the frozen legacy cask's original release ZIP and checksum for
Homebrew re-adoption recovery. Preserve the legacy profile, cache/preferences and
Keychain unchanged; no migration/export of their contents is part of this test.

Only after the backup is verified and the user is ready:

```sh
brew uninstall --cask softmaxe/tap/whisper
# Stop unless /Applications/Whisper.app is now absent.
```

This is normal cask uninstall, never --zap or --force. The inspected cask has no
custom uninstall hooks, only app removal and a separate unused zap stanza. See
[uninstall options](https://docs.brew.sh/Manpage#uninstall-remove-rm-options-installed_formulainstalled_cask-).

## 3. Create a local-only acceptance cask and install A

Use a new local tap, no Git initialization or remote:

```sh
brew tap-new --no-git local/whisper-acceptance
WHISPER_TAP="$(brew --repository local/whisper-acceptance)"
mkdir -p "$WHISPER_TAP/Casks"
WHISPER_CASK="$WHISPER_TAP/Casks/whisper-native-acceptance.rb"
```

Stop if that tap/token already exists; do not overwrite another test or user's
cask. The cask has no pre/postflight, zap, background update or quarantine bypass.
Current local Homebrew maps macOS 27 to `:golden_gate`.

Cask template, with a real file URI and the exact signed ZIP checksum substituted:

```ruby
cask "whisper-native-acceptance" do
  version "A_VERSION"
  sha256 "A_ZIP_SHA256"
  url "file:///ABSOLUTE/URI-ENCODED/PATH/whisper-A_VERSION-macos-arm64.zip"
  name "Whisper native acceptance"
  desc "Private native Whisper upgrade acceptance"
  homepage "https://github.com/softmaxe/whisper"
  depends_on arch: :arm64
  depends_on macos: :golden_gate
  app "Whisper.app"
end
```

Generate the URI with Node's `pathToFileURL`, not manual escaping of spaces. For
example, this future generator writes one version of the private cask:

```sh
node - "$WHISPER_CASK" "$WHISPER_ACCEPT/artifacts/whisper-$WHISPER_A_VERSION-macos-arm64.zip" "$WHISPER_A_VERSION" <<'NODE'
const fs = require('node:fs');
const {createHash} = require('node:crypto');
const {pathToFileURL} = require('node:url');
const [cask, zip, version] = process.argv.slice(2);
if (!/^\d+\.\d+\.\d+$/.test(version)) throw new Error('Invalid acceptance version');
const sha = createHash('sha256').update(fs.readFileSync(zip)).digest('hex');
fs.writeFileSync(cask, `cask "whisper-native-acceptance" do\n  version ${JSON.stringify(version)}\n  sha256 "${sha}"\n  url ${JSON.stringify(pathToFileURL(zip).href)}\n  name "Whisper native acceptance"\n  desc "Private native Whisper upgrade acceptance"\n  homepage "https://github.com/softmaxe/whisper"\n  depends_on arch: :arm64\n  depends_on macos: :golden_gate\n  app "Whisper.app"\nend\n`);
NODE
brew install --cask --require-sha --appdir=/Applications local/whisper-acceptance/whisper-native-acceptance
```

Fully qualified install grants trust to that Homebrew item, not global macOS
certificate trust. Do not disable Homebrew trust enforcement. Source:
[Homebrew Tap Trust](https://docs.brew.sh/Tap-Trust).

Record Homebrew's installed version/receipt, canonical app path, CFBundleVersion,
CFBundleShortVersionString, signature verification and quarantine metadata. Use
`xattr -p com.apple.quarantine /Applications/Whisper.app` only to read it; never
remove or rewrite the attribute. Homebrew's file downloads still use its normal
quarantine path. This local file-origin test must not be described as a published
HTTPS download test.

## 4. Establish A's real permission and credential baseline

Use one new isolated native profile for BOTH A and B:

```sh
WHISPER_ACCEPT_PROFILE="$WHISPER_ACCEPT/native-profile"
open -n /Applications/Whisper.app --args --profile "$WHISPER_ACCEPT_PROFILE"
```

Do not reuse/import the legacy profile or delete an existing default native profile.
Grant normal first-use prompts for A only with the user present. Record each prompt
category/time/outcome separately: Gatekeeper, microphone, Accessibility, native
Keychain and any local-network prompt. Approving an app's normal first launch is
not permission to change certificate trust, disable Gatekeeper or reset TCC.

Configure the intended ASR and cleanup servers in native Settings, with new
synthetic acceptance credentials if possible. Dictate using the real built-in
input, inspect genuine audio/readiness, and confirm automatic insertion into the
disposable target. Repeat with wireless iPhone. Verify saved credentials by an
actual authenticated request; a saved-key indicator alone is insufficient.
Capture only redacted outcomes, not keys, speech, target-window titles or private
device labels. Full-feature/latency acceptance remains #55 evidence.

Quit A normally and verify it is no longer running before changing the cask.
This matters: current Homebrew can reopen apps it quit by bundle ID, which would
lose `--profile` arguments. Do not let an automatic reopen silently use another
profile. Source: [upgrade/reopen implementation](https://github.com/Homebrew/brew/blob/2f1c682db046d37c4b6c09aa43837be6ff270c39/Library/Homebrew/cask/upgrade.rb#L350).

## 5. Upgrade A to B through Homebrew, without a release

Regenerate the SAME cask file with B version, B file URI and B ZIP SHA using the
generator above with the B variables. Do not create a second token or use
reinstall as a substitute for upgrade.

```sh
brew outdated --cask --verbose local/whisper-acceptance/whisper-native-acceptance
brew upgrade --cask --dry-run local/whisper-acceptance/whisper-native-acceptance
brew upgrade --cask --require-sha local/whisper-acceptance/whisper-native-acceptance
brew list --cask --versions whisper-native-acceptance
```

Require an actual A→B transition in Homebrew output and installed receipt, exact
same `/Applications/Whisper.app` path, B plist version, known B archive checksum,
and pinned main/helper signature requirements. If it says already current,
recovery failed or signer changed, stop and preserve the evidence. Do not use
force/reinstall, edit receipts, delete permissions or change trust to manufacture
an upgrade pass. Homebrew snapshots signer/quarantine approval during an upgrade;
that mechanism is distinct from TCC and Keychain outcomes.
[Upgrade implementation](https://github.com/Homebrew/brew/blob/2f1c682db046d37c4b6c09aa43837be6ff270c39/Library/Homebrew/cask/upgrade.rb#L420).

Launch B explicitly with the exact same `--profile` command. Before re-saving or
re-entering any credential, repeat real recording, automatic paste and both saved
credential requests. Record whether each permission category prompts again,
whether each operation succeeds, and the actual macOS/Homebrew/A/B build identity.
A renewed prompt is an observation/failure to diagnose, never something to erase
from the test by resetting TCC or recreating Keychain data.

## Recovery, cleanup and daily-use decision

If any prerequisite or B acceptance fails: quit native, normally uninstall only
the acceptance cask, and restore the verified legacy backup ZIP to `/Applications`.
Re-verify its recorded identity before launching. Legacy profile/credentials remain
untouched and no data import is required.

```sh
brew uninstall --cask local/whisper-acceptance/whisper-native-acceptance
# Stop unless /Applications/Whisper.app is absent.
/usr/bin/ditto -x -k "$WHISPER_ACCEPT/legacy/Whisper-installed-backup.zip" /Applications
codesign --verify --deep --strict /Applications/Whisper.app
```

To restore production Homebrew registration, first ensure the current production
tap cask still exactly matches the frozen 1.0.5 definition, then use
`brew install --cask --adopt softmaxe/tap/whisper` with the preserved original
archive/cache. This must not upgrade the restored app. Homebrew's adopt check
uses bundle versions for apps, so independently compare the restored signatures
and recorded CDHashes; do not treat adopt alone as integrity evidence. If metadata
re-adoption cannot complete safely, keep the verified manual legacy app usable
and report the registration repair as separate work, rather than using force.

After a successful test, the user may keep B under the private acceptance cask for
native daily use only after all acceptance gates pass. Keep its local tap/archive
until transitioning ownership deliberately to the production cask in a separately
authorized release/install operation. Otherwise perform the recovery steps above.
For daily native use, configure its fresh default WhisperNative profile separately
if the acceptance profile was isolated; do not copy legacy data or interpret a new
credential's first-use prompt as an A→B retention failure.

Once the acceptance cask is uninstalled and evidence/recovery archives are safely
retained, `brew untap local/whisper-acceptance` removes the local tap. Remove the
two build worktrees with `git worktree remove` after restoring only their owned
temporary package.json/package-lock.json version edits. Never discard other edits.
Delete only the task-owned acceptance profile/artifacts when no longer needed;
keep the legacy recovery ZIP until the user is satisfied with daily native use.
Do not delete Keychain items during the test. Any later cleanup of exact synthetic
credentials is separate and must not be represented as permission-retention proof.

## Evidence and current limits

Keep four independent results: automated signatures/archive checks; real Homebrew
A→B/canonical-path installation; actual mic/AX/native-Keychain behavior; and full
feature/performance plus daily-use acceptance. Record Homebrew defaults/quarantine,
OS, source SHAs, versions, hashes, helper inventory and all prompts/failures.
Neither passing signatures nor a local cask receipt alone completes #57.

No new product support is demonstrably required for this path. Version overrides
can stay temporary worktree edits, and --profile already provides fresh isolated
native state. The public cask currently describes legacy 1.0.5/macOS 12; updating
its native version/minimum OS/checksum belongs to separately authorized publication,
not this private acceptance plan. Remaining blockers are complete accepted builds,
attended real permission/device testing and the daily-use decision, not a need to
invent a new installer, signing identity, updater or migration feature.
