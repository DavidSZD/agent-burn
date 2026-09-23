import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  assertReleaseTagMatchesVersion,
  assertUpdateFeedDoesNotRegress,
} from "./release-utils.mjs";
import { validateRelease } from "./validate-release.mjs";

test("release tag must match the version in Tauri configuration", () => {
  assert.doesNotThrow(() => assertReleaseTagMatchesVersion("windows-v0.1.59", "0.1.59"));
  assert.throws(
    () => assertReleaseTagMatchesVersion("windows-v0.1.60", "0.1.59"),
    /does not match app version/,
  );
});

test("an older release cannot replace the published update feed", () => {
  assert.doesNotThrow(() => assertUpdateFeedDoesNotRegress("0.1.60", "0.1.59"));
  assert.doesNotThrow(() => assertUpdateFeedDoesNotRegress("0.1.59", "0.1.59"));
  assert.throws(
    () => assertUpdateFeedDoesNotRegress("0.1.58", "0.1.59"),
    /would move the update feed backwards/,
  );
});

test("release validation fails closed when the existing feed file is missing", () => {
  const missingManifest = join(tmpdir(), `agent-burn-missing-${randomUUID()}.json`);

  assert.throws(
    () => validateRelease("windows-v0.1.59", "0.1.59", missingManifest),
    /could not be found/,
  );
});
