// Work that a hidden control panel would lose if main released its renderer:
// requests still in flight, and results that exist only in this window. Main
// releases the panel only while nothing is held.
const holds = new Set<string>();
let reportedRetained = false;

export function setControlPanelHold(reason: string, held: boolean): void {
  if (held) holds.add(reason);
  else holds.delete(reason);

  const retained = holds.size > 0;
  if (retained === reportedRetained) return;
  reportedRetained = retained;
  if (typeof window === "undefined") return;
  window.electronAPI?.setControlPanelRetained?.(retained);
}
