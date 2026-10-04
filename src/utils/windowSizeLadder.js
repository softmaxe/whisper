// Single owner of the pill window's size priority: dictation error > menu >
// toast > compact listening pill > base. A toast dismissing never shrinks the
// window below an active state — higher states always win. The live transcript
// panel sizes the window itself and bypasses this ladder.
export const SIZE_RANK = {
  BASE: 0,
  RECORDING: 1,
  DICTATION_ERROR: 2,
  DICTATION_ERROR_WITH_TRANSCRIPT: 3,
  WITH_MENU: 4,
  WITH_TOAST: 5,
  EXPANDED: 6,
};

export function resolveMainWindowSizeKey({
  menuOpen,
  toastCount,
  compactPill,
  dictationErrorActionCount = 0,
}) {
  if (dictationErrorActionCount > 1) return "DICTATION_ERROR_WITH_TRANSCRIPT";
  if (dictationErrorActionCount === 1) return "DICTATION_ERROR";
  if (menuOpen && (toastCount > 0 || compactPill)) return "EXPANDED";
  if (menuOpen) return "WITH_MENU";
  if (toastCount > 0) return "WITH_TOAST";
  if (compactPill) return "RECORDING";
  return "BASE";
}
