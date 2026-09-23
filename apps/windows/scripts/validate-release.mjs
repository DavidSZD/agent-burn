import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

import {
  assertReleaseTagMatchesVersion,
  assertUpdateFeedDoesNotRegress,
} from "./release-utils.mjs";

export function validateRelease(tag, version, latestManifestPath) {
  assertReleaseTagMatchesVersion(tag, version);
  if (latestManifestPath) {
    if (!existsSync(latestManifestPath)) {
      throw new Error(`The existing update manifest could not be found: ${latestManifestPath}`);
    }
    const manifest = JSON.parse(readFileSync(latestManifestPath, "utf8"));
    if (typeof manifest.version !== "string") {
      throw new Error("The existing update manifest has no valid version.");
    }
    assertUpdateFeedDoesNotRegress(version, manifest.version);
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    const [, , tag, version, latestManifestPath] = process.argv;
    if (!tag || !version) throw new Error("Usage: validate-release.mjs <tag> <version> [latest.json]");
    validateRelease(tag, version, latestManifestPath);
  } catch (error) {
    process.stderr.write(`${String(error)}\n`);
    process.exitCode = 1;
  }
}
