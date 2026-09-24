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
The app selected to receive the text produced by a dictation session: the app
that holds keyboard focus when the Dictation hotkey is pressed. It can differ
from the app shown in the menu bar, as with a floating launcher such as Raycast.

**Automatic paste**:
Insertion of the completed dictation text into the target app without a manual
paste command.

**Dictation hotkey**:
The key, key combination, or mouse button the user assigns to start and stop
Dictation.

**Clean press**:
A press of the Dictation hotkey released without any other key, including
another modifier, or mouse button pressed in between. A press interrupted this
way is void: it neither starts nor stops Dictation, and cancels any Dictation it
started.

**Double tap**:
Two short Clean presses of the Dictation hotkey in quick succession.

**Hold mode**:
The activation mode in which Dictation lasts while the Dictation hotkey is held
and ends when it is released.
_Avoid_: Push-to-talk

**Tap mode**:
The activation mode in which a Double tap starts Hands-free dictation and the
next Clean press ends it. A Dictation hotkey that combines several keys starts
and ends Dictation with a single press instead.

**Hands-free dictation**:
Dictation that continues after the Dictation hotkey is released, until the next
Clean press or cancellation.
