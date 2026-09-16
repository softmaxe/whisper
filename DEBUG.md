# Debug logging

Quit Whisper, then launch the installed app with debug logging:

```sh
/Applications/Whisper.app/Contents/MacOS/Whisper --log-level=debug
```

For a local build, use `dist/mac-arm64/Whisper.app/Contents/MacOS/Whisper` instead.

For persistent logging, add `OPENWHISPR_LOG_LEVEL=debug` to `~/Library/Application Support/whisper/.env` and restart.

Logs are written to `~/Library/Application Support/whisper/logs/debug-*.log`. If `OPENWHISPR_USER_DATA_DIR` is set for a test profile, logs are under that directory instead.

Before sharing logs, remove credentials, private URLs, prompts, transcripts, and personal paths. See [Troubleshooting](TROUBLESHOOTING.md) for common failures and issue reports.

Remove the flag and environment setting, then restart to disable debug logging.
