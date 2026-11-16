# File transcription

Users choose or drop audio/video files, transcribe one or a batch, inspect the text, and open saved results in History.

## Sub-features

- `upload-choose-drop`: file chooser and drop-target entry paths.
- `upload-single-batch`: single and batch progress, final text, History and audio storage.
- `upload-cancel-retry`: cancellation, failed-file retry, reset for another upload.

## How to get to it (user POV)

- Sidebar `Upload` > `Choose Files`, click the drop target, or drag actual files onto it.
- `Speech to Text Settings…` opens server configuration.
- `Transcribe` / `Transcribe N Files` starts work; `Cancel Transcription` / `Cancel All` cancels.
- Success offers `Copy Full Text`, `View in History`, `Upload Another`; batches offer `Retry failed` and `Clear` as appropriate.

## Driving it with cua_repl

Preconditions:

- A disposable known spoken-audio fixture exists and a compatible ASR server/model is configured through Settings. Record fixture path/hash and intended endpoint. History saving is on for the History assertion. Keep fixtures and outputs under the run directory, outside `scratch` if they are evidence.

- **Choose file.** Click `Upload`, inspect AX, then `Choose Files`. Use the native file dialog (Cmd+Shift+G with `await app.pressKey("super+shift+g")` if needed) to select the fixture's absolute path. Require its filename in the queue. Exercise clicking the drop target and actual file drag/drop separately when claiming those paths.
- **Transcribe.** Click `Transcribe`, inspect AX and capture active progress. Require `Transcription complete` and the expected transcript; record actual requests/responses at a production ASR boundary when available, without recording credentials.
- **Side effects.** Click `View in History`, require matching text, query the isolated database and list its `audio` directory before cleanup. Copy the relevant disposable audio/file manifest into evidence. Upload success with History off requires a different expected stored state; do not silently change the assertion.
- **Batch/cancel.** Choose two fixtures and click `Transcribe 2 Files`; capture each queue outcome and stored results. On a separate sufficiently long request activate `Cancel All` (or single `Cancel Transcription`), require processing to stop, and check for partially saved results. Retry a genuine failure with `Retry failed` and inspect the new result; `Upload Another` returns to input state.

## Gotchas

- Baseline has no server/model or audio fixture: successful transcription fails its precondition until supplied. An unconfigured empty view is not a transcription proof.
- MP3, WAV, M4A, WebM, Ogg/Opus, FLAC and AAC can upload directly. Video/other audio may need ffmpeg from `resources/bin`, Homebrew, or PATH; record which executable was used.
- A fake ASR can test UI/storage at the existing HTTP boundary but cannot prove actual recognition quality or compatibility with the intended server. Connection tests alone do not exercise upload.
- Native file dialogs and Finder drag/drop require their own current AX bindings; do not manufacture an internal queued-file object.
