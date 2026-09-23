// Re-reads the local Insights summary whenever the main process reports a new
// analytics event. Refreshes never overlap: one requested mid-run runs once
// after the current pass finishes.
export function subscribeToAnalyticsRefresh(refresh: () => void | Promise<void>): () => void {
  let disposed = false;
  let refreshRunning = false;
  let trailingRefreshRequested = false;

  const runRefresh = async (): Promise<void> => {
    if (disposed) return;
    if (refreshRunning) {
      trailingRefreshRequested = true;
      return;
    }

    refreshRunning = true;
    try {
      await refresh();
    } catch (error) {
      console.error("Refreshing analytics failed:", error);
    } finally {
      refreshRunning = false;
      const runTrailing = trailingRefreshRequested;
      trailingRefreshRequested = false;
      if (!disposed && runTrailing) void runRefresh();
    }
  };

  const disposeLocal = window.electronAPI.onAnalyticsChanged?.(() => void runRefresh());
  void runRefresh();
  return () => {
    disposed = true;
    disposeLocal?.();
  };
}
