# Debug logging

Quit Whisper, then launch the installed app with debug logging:

```sh
/Applications/Whisper.app/Contents/MacOS/Whisper --log-level=debug
```

For a local build, use `dist/mac-arm64/Whisper.app/Contents/MacOS/Whisper` instead.

To keep debug logging enabled across launches, add `OPENWHISPR_LOG_LEVEL=debug` to `~/Library/Application Support/whisper/.env` and restart the app. The environment variable keeps its upstream name.

Logs are written to `~/Library/Application Support/whisper/logs/debug-*.log`. If `OPENWHISPR_USER_DATA_DIR` is set for a test profile, logs are under that directory instead.

Use the logs to inspect microphone capture, audio conversion, Self-Hosted requests, History storage, and paste failures. See [TROUBLESHOOTING.md](TROUBLESHOOTING.md) for checks that apply to this build.

Before sharing log excerpts, remove credentials, private server addresses, prompt text, transcripts, recordings, and personal file paths. Include the application version, macOS version, reproduction steps, and the relevant error.

To disable debug logging, remove the launch flag and the environment setting, then restart Whisper.
