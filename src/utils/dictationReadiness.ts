/**
 * Single owner of the "may a new dictation start?" guard over AudioManager's
 * getState() snapshot, shared by the hotkey toggle, performStartRecording, and
 * the hover-prepare handler in useAudioRecording.js so their checks cannot
 * drift apart.
 */
export interface DictationReadinessState {
  isRecording?: boolean;
  isProcessing?: boolean;
}

export function canStartDictation(state: DictationReadinessState | null | undefined): boolean {
  if (!state) return false;
  return !(state.isRecording || state.isProcessing);
}
