import { create } from "zustand";
import i18n from "../i18n";
import { transcribeFile } from "../services/fileTranscription";
import type { FileTranscriptionConfig } from "../services/fileTranscription";
import { transcriptionErrorKey } from "../components/notes/shared";
import { saveUploadTranscription } from "../services/uploadNotes";
import { setControlPanelHold } from "../utils/controlPanelRetention";

export type QueueItemStatus = "queued" | "transcribing" | "done" | "error";

export interface QueueItem {
  id: string;
  name: string;
  path: string;
  sizeBytes: number;
  status: QueueItemStatus;
  progress: number;
  error?: string;
  // Keep completed text available to copy when local History is disabled.
  text?: string;
  warning?: boolean;
  transcriptionId?: number;
}

export interface TranscribeOptions {
  transcription: FileTranscriptionConfig;
}

interface BatchQueueStoreState {
  queue: QueueItem[];
  isProcessing: boolean;
}

export const useBatchQueueStore = create<BatchQueueStoreState>()(() => ({
  queue: [],
  isProcessing: false,
}));

// Queued, running and finished items live only in this window, and finished
// text stays copyable when History is disabled.
useBatchQueueStore.subscribe((state) => {
  setControlPanelHold("upload-batch", state.isProcessing || state.queue.length > 0);
});

// Self-hosted requests cannot be aborted through the original BYOK transport.
// The run id discards late results while cancellation unlocks the UI immediately.
let runId = 0;
let activeUploadRequestId: string | null = null;

function updateQueue(updater: (prev: QueueItem[]) => QueueItem[]) {
  useBatchQueueStore.setState((s) => ({ queue: updater(s.queue) }));
}

export function addFiles(files: Array<{ name: string; path: string; sizeBytes: number }>) {
  const items: QueueItem[] = files.map((f) => ({
    id: crypto.randomUUID(),
    name: f.name,
    path: f.path,
    sizeBytes: f.sizeBytes,
    status: "queued" as const,
    progress: 0,
  }));
  updateQueue((prev) => [...prev, ...items]);
  return items;
}

export function removeQueueItem(id: string) {
  updateQueue((prev) => prev.filter((item) => item.id !== id));
}

export function cancelBatch() {
  runId++;
  if (activeUploadRequestId) {
    window.electronAPI.cancelUploadTranscription?.(activeUploadRequestId);
    activeUploadRequestId = null;
  }
  useBatchQueueStore.setState((s) => ({
    isProcessing: false,
    queue: s.queue.map((item) =>
      item.status === "done" || item.status === "error"
        ? item
        : { ...item, status: "error" as const, error: "batchCancelled" }
    ),
  }));
}

export function clearBatchQueue() {
  cancelBatch();
  useBatchQueueStore.setState({ queue: [], isProcessing: false });
}

/** Drain the queue independently of React so navigation does not interrupt a batch. */
export function processBatchQueue(transcribeOpts: TranscribeOptions): void {
  if (useBatchQueueStore.getState().isProcessing) return;
  const run = ++runId;
  useBatchQueueStore.setState({ isProcessing: true });

  const snapshotApiKey = transcribeOpts.transcription.getApiKey();
  const transcription: FileTranscriptionConfig = {
    ...transcribeOpts.transcription,
    getApiKey: () => snapshotApiKey,
  };

  const updateItem = (id: string, updates: Partial<QueueItem>) => {
    if (run !== runId) return;
    updateQueue((prev) => prev.map((item) => (item.id === id ? { ...item, ...updates } : item)));
  };

  const processItem = async (item: QueueItem) => {
    try {
      if (run !== runId) return;
      updateItem(item.id, { status: "transcribing", progress: 0 });
      const requestId = crypto.randomUUID();
      activeUploadRequestId = requestId;
      const result = await transcribeFile(item.path, transcription, false, { requestId }).finally(
        () => {
          if (activeUploadRequestId === requestId) activeUploadRequestId = null;
        }
      );
      if (run !== runId) return;

      if (!result.success || !result.text) {
        updateItem(item.id, {
          status: "error",
          error:
            transcriptionErrorKey(result) ||
            (result.messageKey ? i18n.t(result.messageKey) : undefined) ||
            result.error ||
            "batchTranscriptionFailed",
        });
        return;
      }

      updateItem(item.id, { text: result.text, warning: !!result.warning });
      const saved = await saveUploadTranscription(result.text);
      if (saved.success) {
        updateItem(item.id, {
          status: "done",
          progress: 100,
          transcriptionId: saved.id ?? undefined,
        });
      } else {
        updateItem(item.id, { status: "error", error: "errorOccurred" });
      }
    } catch (err) {
      updateItem(item.id, {
        status: "error",
        error:
          transcriptionErrorKey(err) || (err instanceof Error ? err.message : "batchUnknownError"),
      });
    }
  };

  (async () => {
    const processed = new Set<string>();
    let next: QueueItem | undefined;
    while (
      run === runId &&
      (next = useBatchQueueStore
        .getState()
        .queue.find((i) => i.status === "queued" && !processed.has(i.id)))
    ) {
      processed.add(next.id);
      await processItem(next);
    }
    if (run === runId) useBatchQueueStore.setState({ isProcessing: false });
  })();
}

export function useBatchQueue() {
  const { queue, isProcessing } = useBatchQueueStore();
  return {
    queue,
    isProcessing,
    hasQueue: queue.length > 0,
    completedCount: queue.filter((i) => i.status === "done").length,
    failedCount: queue.filter((i) => i.status === "error").length,
    totalCount: queue.length,
    addFiles,
    removeItem: removeQueueItem,
    cancelAll: cancelBatch,
    clearQueue: clearBatchQueue,
    processQueue: processBatchQueue,
  };
}
