// Main asks a newly created control panel to open Settings as soon as the page
// loads, before the lazily loaded panel has mounted its own listener. Listen
// from startup and hold the request until the panel takes it.
let listening = false;
let pending = false;
let handler: (() => void) | null = null;

export function listenForSettingsRequests(): void {
  if (listening) return;
  listening = true;
  window.electronAPI?.onShowSettings?.(() => {
    if (handler) handler();
    else pending = true;
  });
}

export function onSettingsRequested(callback: () => void): () => void {
  handler = callback;
  if (pending) {
    pending = false;
    callback();
  }
  return () => {
    if (handler === callback) handler = null;
  };
}
