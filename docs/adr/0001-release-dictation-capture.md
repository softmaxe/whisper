# Release microphone capture between Dictations

Whisper releases microphone capture when recording stops, startup fails, the user
cancels, or the recording owner is torn down. This decision favors an idle
microphone over retaining a connection to reduce Recording startup latency,
especially for iPhone inputs. Idle hold and background microphone warm-up are
unsupported, including when an older installation saved a nonzero hold duration.

Each request owns its capture until completion or cancellation. A device open that
cannot be aborted must release its stream when it eventually resolves, without
starting recording or producing output for the ended request. Normal completion
preserves recorded data, History retention, and the Target app.

See [Issue #24](https://github.com/softmaxe/whisper/issues/24) and the
[startup specification](https://github.com/softmaxe/whisper/issues/22).
