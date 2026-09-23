export function createUpdateOperationCoordinator() {
  let checkInProgress = false;
  let installInProgress = false;
  let installRequestedAfterCheck = false;

  return {
    requestCheck({ installAfterCheck = false } = {}) {
      if (installInProgress) return "installing";
      if (checkInProgress) {
        installRequestedAfterCheck ||= installAfterCheck;
        return "queued";
      }
      checkInProgress = true;
      installRequestedAfterCheck = installAfterCheck;
      return "started";
    },
    finishCheck(updateAvailable) {
      if (!checkInProgress) return false;
      const shouldInstall = installRequestedAfterCheck && updateAvailable;
      installRequestedAfterCheck = false;
      checkInProgress = false;
      installInProgress = shouldInstall;
      return shouldInstall;
    },
    finishInstall() {
      installInProgress = false;
    },
    isCheckRunning() {
      return checkInProgress;
    },
    isInstallRunning() {
      return installInProgress;
    },
  };
}
