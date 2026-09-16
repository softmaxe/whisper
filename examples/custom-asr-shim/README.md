# Custom ASR shim

Use this local adapter when your ASR server does not support Whisper's Self-Hosted request format.

The shim accepts multipart POST requests at `/audio/transcriptions` and `/v1/audio/transcriptions`. It requires `file` and accepts optional `model`, `language`, and `prompt` fields. It converts audio with FFmpeg, calls the vendor API, and returns `{"text": "..."}`.

Keep vendor credentials in the shim's environment. Whisper's Self-Hosted transport does not send an API-key header.

## Run

Requires Python 3.8 or later and FFmpeg on `PATH`. No Python packages are required. Run commands from `examples/custom-asr-shim/`.

For another vendor, implement `transcribe()` in `shim_template.py`, then run:

```sh
uv run python shim_template.py
```

For the included StepAudio 2.5 adapter, set `STEP_API_KEY` in your shell environment, then run:

```sh
uv run python stepaudio_shim.py
```

In Settings > Speech-to-Text, set the Self-Hosted server URL to `http://localhost:8765`. Supply a model name if your adapter requires it. The StepAudio example fixes the model to `stepaudio-2.5-asr` and language to `zh`; it ignores the forwarded model, language, and prompt fields.

## Tests

The tests stub FFmpeg and the vendor request:

```sh
uv run python test_shim.py
```

## Attribution

Based on [ErogosZhou's StepAudio shim](https://gist.github.com/ErogosZhou/4eb2c4bab1059b404fb652df7bfe24ac), contributed to OpenWhispr.
