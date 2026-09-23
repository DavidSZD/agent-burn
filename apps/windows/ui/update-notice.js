const AVAILABLE_UPDATE_VERSION_KEY = "agent-burn-available-update-version";
const ANNOUNCED_UPDATE_VERSION_KEY = "agent-burn-announced-update-version";
let sessionAvailableUpdateVersion = null;
let sessionAnnouncedUpdateVersion = null;

export function createSafeStorage(getStorage) {
  const fallback = new Map();
  return {
    getItem(key) {
      try {
        const storage = getStorage();
        if (!storage) return fallback.get(key) ?? null;
        const value = storage.getItem(key);
        if (value == null) fallback.delete(key);
        else fallback.set(key, String(value));
        return value ?? null;
      } catch {
        return fallback.get(key) ?? null;
      }
    },
    setItem(key, value) {
      fallback.set(key, String(value));
      try {
        getStorage()?.setItem(key, value);
      } catch {}
    },
    removeItem(key) {
      fallback.delete(key);
      try {
        getStorage()?.removeItem(key);
      } catch {}
    },
  };
}

function normalizeVersion(version) {
  if (typeof version !== "string") return null;
  const normalized = version.trim();
  return /^v?\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/.test(normalized)
    ? normalized
    : null;
}

function compareVersions(left, right) {
  const leftVersion = normalizeVersion(left);
  const rightVersion = normalizeVersion(right);
  if (!leftVersion || !rightVersion) return null;

  const parse = (version) => {
    const withoutBuild = version.replace(/^v/, "").split("+", 1)[0];
    const separator = withoutBuild.indexOf("-");
    return {
      core: (separator < 0 ? withoutBuild : withoutBuild.slice(0, separator)).split(".").map(Number),
      prerelease: separator < 0 ? [] : withoutBuild.slice(separator + 1).split("."),
    };
  };
  const leftParts = parse(leftVersion);
  const rightParts = parse(rightVersion);

  for (let index = 0; index < 3; index += 1) {
    if (leftParts.core[index] !== rightParts.core[index]) {
      return leftParts.core[index] > rightParts.core[index] ? 1 : -1;
    }
  }

  const leftPrerelease = leftParts.prerelease;
  const rightPrerelease = rightParts.prerelease;
  if (leftPrerelease.length === 0 || rightPrerelease.length === 0) {
    if (leftPrerelease.length === rightPrerelease.length) return 0;
    return leftPrerelease.length === 0 ? 1 : -1;
  }

  const identifierCount = Math.max(leftPrerelease.length, rightPrerelease.length);
  for (let index = 0; index < identifierCount; index += 1) {
    const leftIdentifier = leftPrerelease[index];
    const rightIdentifier = rightPrerelease[index];
    if (leftIdentifier === undefined || rightIdentifier === undefined) {
      return leftIdentifier === undefined ? -1 : 1;
    }
    if (leftIdentifier === rightIdentifier) continue;

    const leftNumeric = /^\d+$/.test(leftIdentifier);
    const rightNumeric = /^\d+$/.test(rightIdentifier);
    if (leftNumeric && rightNumeric) {
      return Number(leftIdentifier) > Number(rightIdentifier) ? 1 : -1;
    }
    if (leftNumeric !== rightNumeric) return leftNumeric ? -1 : 1;
    return leftIdentifier > rightIdentifier ? 1 : -1;
  }

  return 0;
}

export function getPersistedAvailableUpdate(storage, currentVersion) {
  let version = sessionAvailableUpdateVersion;
  try {
    if (storage && typeof storage.getItem === "function") {
      version = normalizeVersion(storage.getItem(AVAILABLE_UPDATE_VERSION_KEY));
      sessionAvailableUpdateVersion = version;
    }
  } catch {}

  const comparison = version ? compareVersions(version, currentVersion) : null;
  if (comparison !== null && comparison <= 0) {
    clearPersistedAvailableUpdate(storage);
    return null;
  }
  return version;
}

export function rememberAvailableUpdate(storage, version) {
  const normalized = normalizeVersion(version);
  if (!normalized) return { version: null, shouldAnnounce: false };

  let previouslyAnnounced = sessionAnnouncedUpdateVersion;
  try {
    if (storage && typeof storage.getItem === "function") {
      previouslyAnnounced = storage.getItem(ANNOUNCED_UPDATE_VERSION_KEY) ?? null;
    }
  } catch {}
  const shouldAnnounce = previouslyAnnounced !== normalized;
  sessionAvailableUpdateVersion = normalized;
  if (shouldAnnounce) sessionAnnouncedUpdateVersion = normalized;
  try {
    storage?.setItem(AVAILABLE_UPDATE_VERSION_KEY, normalized);
    if (shouldAnnounce) storage?.setItem(ANNOUNCED_UPDATE_VERSION_KEY, normalized);
  } catch {}

  return { version: normalized, shouldAnnounce };
}

export function clearPersistedAvailableUpdate(storage) {
  sessionAvailableUpdateVersion = null;
  try {
    storage?.removeItem(AVAILABLE_UPDATE_VERSION_KEY);
  } catch {}
}
