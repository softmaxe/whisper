# Whisper

Whisper turns dictated speech into text for the app where the user is working.

## Language

**Dictation**:
A session in which the user records speech, waits for transcription, and receives
the resulting text in the target app.

**Recording readiness**:
The point at which the selected microphone can capture speech without losing
the beginning of the utterance.

**Recording startup latency**:
The interval from requesting Dictation to Recording readiness. It excludes
transcription and Automatic paste time.

**Recording pill**:
The floating indicator for dictation, including recording activity, the speech
waveform, and transcription progress.
_Avoid_: Status bar, menu bar icon

**Target app**:
The app selected to receive the text produced by a dictation session.

**Automatic paste**:
Insertion of the completed dictation text into the target app without a manual
paste command.
