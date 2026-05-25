import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { rmSync, mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";

const repo = process.cwd();
const outDir = join(process.env.TEMP, "meeting-minutes-transcription-queue-test");
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
      "src/lib/transcriptionQueue.ts",
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

const { transcribeChunksConcurrently } = require(join(outDir, "transcriptionQueue.js"));

const chunks = [
  { index: 0, audioPath: "a.flac", startSec: 0, endSec: 10, offsetSec: 0, durationSec: 10 },
  { index: 1, audioPath: "b.flac", startSec: 9, endSec: 20, offsetSec: 9, durationSec: 11 },
  { index: 2, audioPath: "c.flac", startSec: 19, endSec: 30, offsetSec: 19, durationSec: 11 },
];

let active = 0;
let maxActive = 0;
const progress = [];
const expectedOffsets = new Map([
  ["a.flac", 0],
  ["b.flac", 9],
  ["c.flac", 19],
]);

const result = await transcribeChunksConcurrently({
  chunks,
  apiKey: "test-key",
  concurrency: 2,
  transcribeChunk: async (audioPath, apiKey, offsetSec) => {
    assert.equal(apiKey, "test-key");
    assert.equal(offsetSec, expectedOffsets.get(audioPath));
    active += 1;
    maxActive = Math.max(maxActive, active);
    await new Promise((resolve) => setTimeout(resolve, audioPath === "a.flac" ? 30 : 5));
    active -= 1;
    return [{ id: 0, start: offsetSec, end: offsetSec + 1, text: audioPath }];
  },
  onChunkDone: (event) => progress.push(event),
});

assert.equal(maxActive, 2);
assert.deepEqual(result.map((segment) => segment.text), ["a.flac", "b.flac", "c.flac"]);
assert.deepEqual(result.map((segment) => segment.id), [0, 1, 2]);
assert.equal(progress.length, 3);
assert.equal(progress.at(-1).completedChunks, 3);
assert.equal(progress.at(-1).completedAudioSec, 32);
assert.deepEqual(
  progress.map((event) => event.completedChunks),
  [1, 2, 3],
);
assert.deepEqual(
  progress.map((event) => event.completedAudioSec),
  [11, 22, 32],
);

const nanConcurrencyProgress = [];
const nanConcurrencyResult = await transcribeChunksConcurrently({
  chunks,
  apiKey: "test-key",
  concurrency: Number.NaN,
  transcribeChunk: async (audioPath, apiKey, offsetSec) => {
    assert.equal(apiKey, "test-key");
    return [{ id: 0, start: offsetSec, end: offsetSec + 1, text: audioPath }];
  },
  onChunkDone: (event) => nanConcurrencyProgress.push(event),
});

assert.deepEqual(
  nanConcurrencyResult.map((segment) => segment.text),
  ["a.flac", "b.flac", "c.flac"],
);
assert.equal(nanConcurrencyProgress.length, 3);

let emptyProgressCalled = false;
const emptyResult = await transcribeChunksConcurrently({
  chunks: [],
  apiKey: "test-key",
  concurrency: 2,
  transcribeChunk: async () => {
    throw new Error("empty chunks should not be transcribed");
  },
  onChunkDone: () => {
    emptyProgressCalled = true;
  },
});

assert.deepEqual(emptyResult, []);
assert.equal(emptyProgressCalled, false);

const failureProgress = [];
let slowWorkerSawCancellation = false;
await assert.rejects(
  transcribeChunksConcurrently({
    chunks: [
      { index: 0, audioPath: "fail.flac", startSec: 0, endSec: 1, offsetSec: 0, durationSec: 1 },
      { index: 1, audioPath: "slow.flac", startSec: 1, endSec: 2, offsetSec: 1, durationSec: 1 },
    ],
    apiKey: "test-key",
    concurrency: 2,
    transcribeChunk: async (audioPath, apiKey, offsetSec, isCancelled) => {
      assert.equal(apiKey, "test-key");
      if (audioPath === "fail.flac") {
        await new Promise((resolve) => setTimeout(resolve, 5));
        throw new Error("transcription failed");
      }

      await new Promise((resolve) => setTimeout(resolve, 30));
      slowWorkerSawCancellation = isCancelled?.() ?? false;
      return [{ id: 0, start: offsetSec, end: offsetSec + 1, text: audioPath }];
    },
    onChunkDone: (event) => failureProgress.push(event),
  }),
  /transcription failed/,
);

await new Promise((resolve) => setTimeout(resolve, 40));
assert.equal(failureProgress.length, 0);
assert.equal(slowWorkerSawCancellation, true);

const sortedResult = await transcribeChunksConcurrently({
  chunks: [
    { index: 0, audioPath: "early-chunk.flac", startSec: 0, endSec: 10, offsetSec: 0, durationSec: 10 },
    { index: 1, audioPath: "overlap.flac", startSec: 8, endSec: 18, offsetSec: 8, durationSec: 10 },
    { index: 2, audioPath: "late.flac", startSec: 18, endSec: 28, offsetSec: 18, durationSec: 10 },
  ],
  apiKey: "test-key",
  concurrency: 3,
  transcribeChunk: async (audioPath) => {
    if (audioPath === "early-chunk.flac") {
      return [{ id: 99, start: 9, end: 10, text: "chunk-zero-later-start" }];
    }
    if (audioPath === "overlap.flac") {
      return [{ id: 77, start: 8.5, end: 9.5, text: "chunk-one-earlier-start" }];
    }
    return [{ id: 55, start: 20, end: 21, text: "chunk-two-late-start" }];
  },
});

assert.deepEqual(
  sortedResult.map((segment) => segment.text),
  ["chunk-one-earlier-start", "chunk-zero-later-start", "chunk-two-late-start"],
);
assert.deepEqual(sortedResult.map((segment) => segment.id), [0, 1, 2]);
