# Custom ASR shim

These retained OpenWhispr examples adapt a vendor's ASR API to Whisper's Self-Hosted transcription path. Use them only when your server does not accept multipart audio at `/audio/transcriptions` and return JSON with a `text` field.

The shim accepts `/audio/transcriptions` and `/v1/audio/transcriptions`, converts audio with FFmpeg, calls the configured vendor, and returns `{"text": "..."}`. The application continues using its existing Self-Hosted request implementation.

## Request fields

| Field      | Purpose                                         |
| ---------- | ----------------------------------------------- |
| `file`     | Required audio file, such as WebM/Opus or WAV.  |
| `model`    | Model name configured in Settings, if supplied. |
| `language` | Selected transcription language, if supplied.   |
| `prompt`   | Dictionary hints, if supplied.                  |

Keep vendor credentials in the shim's environment. The upstream Self-Hosted upload transport does not send an API-key header. This path returns one JSON response rather than an SSE stream.

## Run

Requires Python 3.8 or later and FFmpeg on `PATH`. The examples use only the Python standard library.

For another vendor, implement `transcribe()` in `shim_template.py`, then run:

```sh
uv run python shim_template.py
```

For the included StepAudio 2.5 adapter, set `STEP_API_KEY` in your shell environment, then run:

```sh
uv run python stepaudio_shim.py
```

In Whisper's Settings > Speech-to-Text, set the Self-Hosted server URL to `http://localhost:8765` and supply a model name if your adapter requires it. The StepAudio example uses Chinese by default and ignores the forwarded language and model fields; adjust its vendor request when needed.

## Tests

The tests stub FFmpeg and the vendor request:

```sh
uv run python test_shim.py
```

## Attribution

Based on [ErogosZhou's StepAudio shim](https://gist.github.com/ErogosZhou/4eb2c4bab1059b404fb652df7bfe24ac), contributed to OpenWhispr.
