function parseStableVersion(version) {
  const match = /^(\d+)\.(\d+)\.(\d+)$/.exec(version);
  if (!match) throw new Error(`Expected a stable x.y.z version, received: ${version}`);
  return match.slice(1).map(Number);
}

export function compareStableVersions(left, right) {
  const leftParts = parseStableVersion(left);
  const rightParts = parseStableVersion(right);
  for (let index = 0; index < leftParts.length; index += 1) {
    if (leftParts[index] !== rightParts[index]) return Math.sign(leftParts[index] - rightParts[index]);
  }
  return 0;
}

export function assertReleaseTagMatchesVersion(tag, version) {
  if (tag !== `windows-v${version}`) {
    throw new Error(`Release tag ${tag} does not match app version ${version}`);
  }
}

export function assertUpdateFeedDoesNotRegress(incomingVersion, publishedVersion) {
  if (compareStableVersions(incomingVersion, publishedVersion) < 0) {
    throw new Error(
      `Release ${incomingVersion} would move the update feed backwards from ${publishedVersion}`,
    );
  }
}
