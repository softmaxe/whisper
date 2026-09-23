/** Set when the user proceeds past macOS Accessibility without granting. Silences the nag and enables clipboard-only paste. */
export const ACCESSIBILITY_SKIPPED_KEY = "accessibilitySkipped";

export function isAccessibilitySkipped(): boolean {
  if (typeof window === "undefined") return false;
  return window.localStorage.getItem(ACCESSIBILITY_SKIPPED_KEY) === "true";
}
