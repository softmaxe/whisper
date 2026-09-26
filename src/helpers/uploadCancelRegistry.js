// In-flight file uploads keyed by requestId. Each signal covers preparation
// and the self-hosted transcription request for that upload.
function createUploadCancelRegistry() {
  // requestId -> Set<AbortController>, one controller per in-flight operation.
  const controllers = new Map();

  return {
    // Callers without a valid requestId get no signal and a no-op release.
    register(requestId) {
      if (typeof requestId !== "string" || !requestId) {
        return { signal: undefined, release: () => {} };
      }
      let set = controllers.get(requestId);
      if (!set) {
        set = new Set();
        controllers.set(requestId, set);
      }
      const controller = new AbortController();
      set.add(controller);
      return {
        signal: controller.signal,
        release: () => {
          set.delete(controller);
          // Guard against deleting a successor set registered after a cancel.
          if (set.size === 0 && controllers.get(requestId) === set) {
            controllers.delete(requestId);
          }
        },
      };
    },

    // Aborts every operation registered under the id and returns how many were
    // aborted. Unknown or already-finished ids are a safe no-op.
    cancel(requestId) {
      const set = controllers.get(requestId);
      if (!set) return 0;
      controllers.delete(requestId);
      for (const controller of set) controller.abort();
      return set.size;
    },
  };
}

module.exports = { createUploadCancelRegistry };
