# Resume native migration acceptance

Continue specification #35 on `codex/native-macos-spec` and draft PR #58.
Do not restart the migration or replace the installed application before the
remaining acceptance gates pass. The implementation uses a fresh native profile;
the existing legacy profile and credentials are not migration inputs.

## Next interactive session

1. Check the branch, working tree, PR head and latest CI result. Preserve the
   pre-existing edits to `CONTEXT.md` and the untracked native migration ADR.
2. Read `docs/native-migration-status.md`, `docs/native-acceptance-map.md`,
   `docs/native-performance-baseline.md`, `native/README.md` and the release smoke
   procedure in `test/README.md`. Use the exact tested package and source revision
   recorded in the status document, rebuilding after any source change.
3. Reserve the desktop and physical input session. Start with the signed native
   bundle and a disposable profile. Complete synthetic English/Chinese UI review
   and any source follow-ups recorded in the status document.
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
7. Follow the actual two-version signed Homebrew upgrade smoke check. Keep the
   original certificate/private key and app/helper identifiers. Observe microphone,
   Accessibility and Keychain behavior; signature equality alone is insufficient.
8. After full feature, interface, performance and upgrade acceptance, remove the
   AGENTS rule requiring upstream OpenWhispr inspection for every change, mark
   PR #58 ready, and decide the daily-use transition. Publishing needs a separate
   request.

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
