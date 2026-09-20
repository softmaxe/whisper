# Resume native migration acceptance

Continue specification #35 on `codex/native-macos-spec` and draft PR #58.
Do not restart the migration or replace the installed application before the
remaining acceptance gates pass. The implementation uses a fresh native profile;
the existing legacy profile and credentials are not migration inputs.

Source work is complete at `8770993a460eba252dce91a77274327ee436799f`, where the
automated gates in [the status document](native-migration-status.md) pass. The
signed package built from that revision is `dist/native-arm64/Whisper.app` with
`release/whisper-1.0.5-macos-arm64.zip` and its checksum; `npm run pack:release`
rebuilds and re-verifies it against the pinned certificate. The installed 1.0.5
application and its profile were not touched.

## Next interactive session

1. Check the branch, working tree, PR head and the latest CI result. Preserve the
   pre-existing edits to `CONTEXT.md` and the untracked
   `docs/adr/0002-native-macos-application.md`; decide separately whether they
   belong in the migration commit.
2. Read `docs/native-migration-status.md`, `docs/native-acceptance-map.md`,
   `docs/native-performance-baseline.md`, `native/README.md` and the release smoke
   procedure in `test/README.md`. Use the exact tested package and source revision
   recorded in the status document, rebuilding after any source change.
3. Reserve the desktop and physical input session. Start with the signed native
   bundle and a disposable profile. The shell and recovery review is already done
   in English and Chinese through `--synthetic-preview`, `--preview-pill` and
   `--preview-recovery`; retest with a physical keyboard the two paths that did not
   respond to automation, Return on a selected search result and standard
   multi-file selection in the Open panel.
4. Test built-in microphone and wireless iPhone independently. Check first spoken
   words, readiness feedback, Right Command hold/double tap/standalone stop,
   ordinary Command combinations, cancellation in every phase, capture release,
   disconnection and immediate retry. Exercise long Hands-free Dictation and
   target-app changes, then verify actual field insertion and clipboard recovery.
5. Check real server ASR and cleanup success/fallback; History retry/retention;
   Dictionary, correction learning/Undo and Snippets; individual/batch Upload
   with History on/off; Insights exclusions; desktop preferences and permissions.
6. Collect the packaged baseline and native performance samples under matching
   conditions. Use at least 20 valid starts per compared input/condition and
   separate cold-launch trials. Keep failures, cancellations and missing samples
   visible. Set baseline-derived budgets before deciding whether native meets
   them. Do not treat deterministic test timings as physical measurements.
7. Follow the [private two-version Homebrew procedure](native-upgrade-acceptance.md)
   and the release smoke check. Keep the
   original certificate/private key and app/helper identifiers. Observe microphone,
   Accessibility and Keychain behavior; signature equality alone is insufficient.
8. After full feature, interface, performance and upgrade acceptance, remove the
   AGENTS rule that requires inspecting upstream OpenWhispr for every change (the
   user asked for this removal once the migration is complete) and decide the
   daily-use transition. PR #58 was marked ready for review on request on
   September 20, 2026. Publishing needs a separate request.

## Open observations from the last synthetic pass

- Return or keypad Enter on a selected search result, and standard multi-file
  selection in the Open panel, did not respond through the UI automation tool. The
  cause is not established. Retest with a physical keyboard before changing
  product behavior, and treat both as unverified rather than failed.
- Packaged samples, the baseline-derived budgets, physical input behavior,
  permission retention and daily-use readiness have no evidence yet. Every
  claim in [the acceptance map](native-acceptance-map.md) that names `UI`, `HW`,
  `PERF`, `PKG` or `UPGRADE` is still open.

## Recovery rules

- A failed physical or server check resumes from its recorded failing condition;
  it does not invalidate unrelated automated results. Diagnose and fix it, rerun
  the affected checks, and rebuild the signed artifact before accepting that fix.
- If signing credentials are missing or mismatched, preserve the current package
  and restore the original credentials. Do not generate a replacement identity.
- Do not reset TCC, alter system trust, delete Keychain items or remove the legacy
  profile to make an upgrade test pass.
- Keep trace exports local and review them before publication. Evidence must not
  include speech, transcripts, private device labels, server addresses or keys.

The user can resume by saying: "Continue native migration from the recorded
checkpoint and start the remaining acceptance checks."
