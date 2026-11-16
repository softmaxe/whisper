# History, search, and Insights

Users inspect saved dictations/uploads, compare raw and cleaned text, copy or retry audio, search transcripts, and see dictation activity statistics.

## Sub-features

- `history-read-copy`: open saved text, copy, and expand differing raw text.
- `history-search`: toolbar and Cmd+K search, matching/empty results, Return to a result.
- `history-retry`: transcribe retained audio again.
- `history-delete`: delete/Undo, deadline commit, close/reopen persistence.
- `insights-period`: inspect dictation totals and period comparisons.

## How to get to it (user POV)

- Sidebar `History`; each transcript row exposes `Copy text`, optional `Show raw transcript`, and `More actions`.
- Toolbar search or `Cmd+K` opens `Search transcripts`.
- Row `More actions` > `Re-transcribe`, `Reveal in Finder`, or `Delete`; keyboard row Return copies, Space expands raw text, Cmd+Backspace/Delete removes.
- Sidebar `Insights` opens aggregate dictation statistics.

## Driving it with cua_repl

Preconditions:

- Produce a known transcript through [Upload](upload.md) or [dictation](dictation.md), with History saving enabled. Retry also needs retained audio and reachable ASR. Insights activity needs real dictation; an upload alone does not establish dictation analytics.

- **Inspect/copy.** Click `History`, inspect AX, select the known transcript. Click `Copy text` or focus its row and `await app.pressKey("Return")`. Paste into a disposable editor to observe the clipboard result; preserve/restore the user's clipboard if the tooling permits. Capture action and result, plus the read-only History count.
- **Raw text.** Click `Show raw transcript`, inspect AX and require the actual raw text, then `Hide raw transcript`. When raw and final text are identical the control is absent and this sub-feature lacks its required fixture.
- **Search entries.** `await app.pressKey("super+k")`, inspect AX, type a known substring into `Search transcripts`, and require its result. Return should close search and highlight that History item. Separately use the visible toolbar search button. Replace the query with `VerificationNoMatchQuokka` to require `No results`; `Clear search` empties it and Escape closes search.
- **Retry.** Choose `More actions` > `Re-transcribe`; capture request progress and the resulting text. Verify retained audio existed before the action and query stored state afterward. A connection-test success is insufficient.
- **Delete/Undo.** Choose `Delete` or the row keyboard shortcut; require `Transcript deleted` and `Undo`. Activate Undo before six seconds and require the row. Repeat without Undo, then close/reopen the panel after the deadline and require the stored count to decrease. Never run this on the installed database.
- **Insights.** Click `Insights`, choose the visible period control from the fresh AX tree, and compare its rendered totals to read-only `analytics_events` for the selected dates. Save both periods' screenshots/data; an empty panel is only empty-state evidence.

## Gotchas

- Fresh data has no transcripts, retained audio, or analytics: populated search, raw toggle, retry, deletion, and nonempty Insights journeys fail until real actions create their prerequisites. Do not seed SQLite to bypass this.
- History saving/retention settings can remove prerequisites. A hidden row during Undo is not yet a committed deletion.
- `Reveal in Finder` opens another app; close only the window the run opened. Do not mistake files from a different instance for proof.
