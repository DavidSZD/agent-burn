export function createUpdateScanShutdown() {
  let pending = false;

  return {
    isStopping() {
      return pending;
    },

    clearTimelineRefreshForUpdate(setTimelineRefreshing) {
      if (!pending) return false;
      setTimelineRefreshing(false, "timeline");
      return true;
    },

    async run({ prepare, install, resume, onStart, onResume }) {
      if (pending) return false;
      pending = true;
      onStart?.();
      let handedOff = false;
      try {
        await prepare();
        handedOff = (await install()) === true;
        return handedOff;
      } finally {
        if (!handedOff) {
          try {
            await resume();
          } finally {
            pending = false;
            onResume?.();
          }
        }
      }
    },
  };
}
