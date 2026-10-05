# The renderer owns Dictation state

One renderer module owns each Dictation's state, locks, and side effects,
because microphone capture, transcription, and Automatic paste results all
happen in the renderer. The main process keeps a read-only record of the
current Dictation for hotkey gating and the tray. Main mints a `dictationId`
for each Dictation, together with its Target app, when the Dictation hotkey is
pressed. Every lifecycle report from the renderer carries that id, and main
discards any report for a Dictation that is no longer current. Automatic paste
uses the Target app recorded for the Dictation, not whichever app was captured
most recently.

This replaces state that was mirrored in about eight places in both processes
and reported by hand from many call sites. In that design, a missed report
could leave main stuck in "preparing".

## Considered options

- **Main owns the state and the renderer executes commands.** Hotkey gating
  would not wait on IPC, but every capture, transcription, and delivery event
  would have to travel back to main to cause a transition.
- **Both processes keep the state and synchronize it.** This is the earlier
  design, and the same structure as upstream OpenWhispr; it is the source of
  the mirror-drift bugs.

## Consequences

Main sees a transition one IPC hop late. Between main sending
`prepare-dictation` and the renderer reporting "preparing", main treats the
Dictation as requested, so a second press stops it. The `useAudioRecording`
lifecycle no longer tracks upstream OpenWhispr line for line, so upstream fixes
have to be ported by hand.
