import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { rmSync, mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";

const repo = process.cwd();
const outDir = join(process.env.TEMP, "meeting-minutes-processing-chunks-test");
const require = createRequire(import.meta.url);

rmSync(outDir, { recursive: true, force: true });
mkdirSync(outDir, { recursive: true });

const compile = spawnSync(
  "cmd.exe",
  [
    "/d",
    "/s",
    "/c",
    [
      "npx",
      "tsc",
      "src/lib/processingChunks.ts",
      "--target",
      "ES2021",
      "--module",
      "CommonJS",
      "--moduleResolution",
      "node",
      "--skipLibCheck",
      "--outDir",
      outDir,
    ].join(" "),
  ],
  { cwd: repo, encoding: "utf8" },
);

assert.equal(compile.status, 0, compile.stdout + compile.stderr);

const { resolveSegmentsForFactScheduling } = require(join(outDir, "processingChunks.js"));

const baseChunk = {
  meetingId: "meeting-1",
  index: 0,
  audioPath: "chunk.flac",
  startSec: 0,
  endSec: 10,
  offsetSec: 0,
  durationSec: 10,
  rawSegmentsJson: null,
  errorMsg: null,
  factsStatus: "pending",
  factsJson: null,
  factsErrorMsg: null,
};

let parserCalls = 0;
const parseStoredSegments = () => {
  parserCalls += 1;
  throw new Error("parser should not run for non-done chunks");
};

assert.equal(
  resolveSegmentsForFactScheduling(
    { ...baseChunk, status: "pending" },
    undefined,
    parseStoredSegments,
  ),
  null,
);
assert.equal(parserCalls, 0);

const knownSegments = [{ id: 0, start: 0, end: 2, text: "ok" }];
assert.deepEqual(
  resolveSegmentsForFactScheduling(
    { ...baseChunk, status: "done", rawSegmentsJson: JSON.stringify(knownSegments) },
    knownSegments,
    parseStoredSegments,
  ),
  knownSegments,
);
assert.equal(parserCalls, 0);

const parsedSegments = [{ id: 1, start: 3, end: 4, text: "parsed" }];
assert.deepEqual(
  resolveSegmentsForFactScheduling(
    { ...baseChunk, status: "done", rawSegmentsJson: JSON.stringify(parsedSegments) },
    undefined,
    () => parsedSegments,
  ),
  parsedSegments,
);
