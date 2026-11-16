# Dictionary and snippets

Users teach Whisper names and terms, import word lists, and define spoken triggers that expand to saved text. Editing these definitions is available offline; proving their effect on transcription requires live dictation.

## Sub-features

- `words-add`: empty-state and header add paths save manual words.
- `words-import-export`: import comma/newline lists and export the full dictionary through its page menu, regardless of the current filter.
- `words-filter-edit`: filter words, edit a row, and reject duplicates.
- `words-remove-undo`: remove a row and undo within its deadline.
- `snippets-create-edit`: create, filter, edit, and remove trigger/replacement definitions.
- `dictionary-influence`: actual dictation uses words/snippets; Upload and History retry do not increment dictionary usage.

## How to get to it (user POV)

- Sidebar `Dictionary` > `Words`; choose `Add Your First Word` or `Add Word`.
- Empty state `Import a List`, or `Dictionary actions` > `Import List…`; populated page menu also offers `Export…`, `Dictionary Settings…`, and `Clear Dictionary…`.
- Search with `Filter words`, or use `Add “<query>”` from the no-match result.
- Switch to `Snippets` > `New Snippet` (header or empty-state action).
- Focus a word/snippet row and press Return to edit or Delete to remove; row actions/context menus offer editing and removal too.

## Driving it with cua_repl

Preconditions:

- Baseline instance is healthy. No word equals `Verification Quokka`.
- Read the shared index for resolving each `index`; never reuse indices from a previous observation.

- **Reach empty Dictionary.** `await app.click(index)` on `Dictionary`, then `await app.pressKey("Tab"); await app.getAXState({disableDiffing:true});`. Require `Your dictionary is empty` in the screenshot and Words/Snippets/add controls in AX; capture `before` evidence, then emit a fresh full tree for new indices.
- **Empty-state add.** Click `Add Your First Word`, send Tab and inspect a fresh full AX tree, then click the `Add words, separated by commas` field and `await app.typeText("Verification Quokka")`. Capture `action` evidence with the entered value before submitting.
- **Save.** `await app.pressKey("Return"); await app.getAXState();` Require the row `Verification Quokka` and `Added 1 word` feedback (toast may expire). Alternatively activate the add panel's `Add` button; mark the actual entry point used.
- **Stored proof.** Run the skill's `state` command. Require exactly one matching dictionary row with source `manual` and use count `0`; no transcripts or analytics should have been created by this offline action. Capture `after` evidence.
- **Reopen.** Click `History`, send Tab and inspect AX; click `Dictionary`, send Tab and inspect AX. Require the same word row and capture `reopened` evidence. This verifies leaving/returning to the view plus persisted SQLite state, not a process-restart journey.
- **Header add and duplicate.** On the populated page, click `Add Word`, type `Verification Otter`, and Return. Re-enter `verification quokka`; require duplicate feedback and still one case-insensitive matching DB row. This is a separate path from the initial proof.
- **Filter/edit/remove.** Click the `Filter words` input and type `Quokka`; require only the matching row. Focus that row and Return, replace its word with `Verification Quokka Revised`, then Return. Read DB state. Focus the row and Delete; require removal and `Undo`, activate `Undo` and require restoration. To prove committed deletion, repeat without Undo and wait for the six-second deadline, then query state.
- **Import/export.** Click the stated empty-state or page-menu Import entry, paste `Verification Finch\nVerification Heron` in the import field using `await app.paste("Verification Finch\nVerification Heron")`, and activate `Import`. Require two DB rows. Use `Dictionary actions` > `Export…`, save through the native dialog to `$VERIFY_RUN/export.txt`, and read that file to verify the expected words. Cover each entry separately.
- **Snippet definition.** Click `Snippets`, `New Snippet`; type `verification signoff` in `e.g. cal link` and `Regards, Verification` in the replacement field. Choose `Create Snippet` or `await app.pressKey("super+Return")`. Require the visible row plus matching `snippets` state. Reopen it with Return, edit and save; verify both values again. Filtering uses `Filter snippets`. These actions do not prove spoken expansion.

## Gotchas

- Empty state exposes two add controls; label the entry actually tested. Header, no-match Add, import, context menu, and keyboard paths remain separate coverage.
- GPUI text inputs may appear as multiple AX nodes; click the current editable field, not its label. Adding words keeps the panel open and clears its input.
- An empty Dictionary has no filter field or page menu. Import/export use native clipboard/save UI and must stay within scratch/evidence destinations.
- Undo has a six-second deadline. Clear Dictionary is destructive and cannot be undone; it is not part of the initial proof.
- Empty baseline cannot prove learned-word filters or dictation influence. Those paths fail their precondition until a real dictation/learning journey produces the needed state.
