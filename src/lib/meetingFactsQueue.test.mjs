import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { rmSync, mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";

const repo = process.cwd();
const outDir = join(process.env.TEMP, "meeting-minutes-facts-queue-test");
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
      "src/lib/meetingFactsQueue.ts",
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

const {
  buildAdaptiveFactBatches,
  extractMeetingFactsConcurrently,
} = require(join(outDir, "meetingFactsQueue.js"));

const cachedFacts = {
  chunkIndex: 0,
  startSec: 0,
  endSec: 10,
  summary: "Resumo em cache",
  topics: ["Cache"],
  decisions: [],
  actions: [],
  questions: [],
  risks: [],
};

const chunks = [
  {
    meetingId: "meeting-1",
    index: 0,
    audioPath: "a.flac",
    startSec: 0,
    endSec: 10,
    offsetSec: 0,
    durationSec: 10,
    status: "done",
    rawSegmentsJson: JSON.stringify([{ id: 0, start: 0, end: 1, text: "cache" }]),
    errorMsg: null,
    factsStatus: "done",
    factsJson: JSON.stringify(cachedFacts),
    factsErrorMsg: null,
  },
  {
    meetingId: "meeting-1",
    index: 1,
    audioPath: "b.flac",
    startSec: 10,
    endSec: 20,
    offsetSec: 10,
    durationSec: 10,
    status: "done",
    rawSegmentsJson: JSON.stringify([{ id: 1, start: 10, end: 11, text: "acao" }]),
    errorMsg: null,
    factsStatus: "pending",
    factsJson: null,
    factsErrorMsg: null,
  },
  {
    meetingId: "meeting-1",
    index: 2,
    audioPath: "c.flac",
    startSec: 20,
    endSec: 30,
    offsetSec: 20,
    durationSec: 10,
    status: "done",
    rawSegmentsJson: JSON.stringify([{ id: 2, start: 20, end: 21, text: "decisao" }]),
    errorMsg: null,
    factsStatus: "error",
    factsJson: null,
    factsErrorMsg: "old error",
  },
];

let active = 0;
let maxActive = 0;
const updates = [];
const progress = [];

const result = await extractMeetingFactsConcurrently({
  chunks,
  apiKey: "gemini-key",
  concurrency: 2,
  parseSegments: (chunk) => JSON.parse(chunk.rawSegmentsJson),
  extractChunkFacts: async (chunk, segments, apiKey) => {
    assert.equal(apiKey, "gemini-key");
    assert.equal(segments.length, 1);
    active += 1;
    maxActive = Math.max(maxActive, active);
    await new Promise((resolve) => setTimeout(resolve, chunk.index === 1 ? 20 : 5));
    active -= 1;
    return {
      chunkIndex: chunk.index,
      startSec: chunk.startSec,
      endSec: chunk.endSec,
      summary: `Resumo ${chunk.index}`,
      topics: [`Topico ${chunk.index}`],
      decisions: [],
      actions: [
        {
          task: `Acao ${chunk.index}`,
          owner: "Ana",
          deadline: "",
          timestampSec: chunk.startSec + 1,
          evidence: "evidencia",
        },
      ],
      questions: [],
      risks: [],
    };
  },
  updateChunkFacts: async (chunk, status, factsJson, errorMsg) => {
    updates.push({ index: chunk.index, status, factsJson, errorMsg });
  },
  onChunkDone: (event) => progress.push(event),
});

assert.equal(maxActive, 2);
assert.deepEqual(result.map((item) => item.chunkIndex), [0, 1, 2]);
assert.deepEqual(result.map((item) => item.summary), ["Resumo em cache", "Resumo 1", "Resumo 2"]);
assert.deepEqual(
  updates.map((item) => [item.index, item.status]),
  [
    [1, "running"],
    [2, "running"],
    [2, "done"],
    [1, "done"],
  ],
);
assert.equal(progress.length, 3);
assert.equal(progress.at(-1).completedChunks, 3);
assert.equal(progress.at(-1).cachedChunks, 1);
assert.equal(progress.at(-1).extractedChunks, 2);

const batchPlan = buildAdaptiveFactBatches({
  chunks: chunks.slice(1),
  parseSegments: (chunk) => JSON.parse(chunk.rawSegmentsJson),
  maxBatchChars: 10,
});

assert.deepEqual(
  batchPlan.map((batch) => batch.items.map((item) => item.chunk.index)),
  [[1], [2]],
);

const batchCalls = [];
const batchUpdates = [];
const batched = await extractMeetingFactsConcurrently({
  chunks: chunks.slice(1),
  apiKey: "gemini-key",
  concurrency: 2,
  parseSegments: (chunk) => {
    const base = JSON.parse(chunk.rawSegmentsJson);
    return [
      ...base,
      { id: 100 + chunk.index, start: chunk.startSec + 1, end: chunk.startSec + 2, text: "" },
      { id: 200 + chunk.index, start: chunk.startSec + 2, end: chunk.startSec + 3, text: base[0].text },
    ];
  },
  maxBatchChars: 80,
  extractFactBatch: async (batch, apiKey) => {
    assert.equal(apiKey, "gemini-key");
    batchCalls.push(batch.map((item) => ({
      index: item.chunk.index,
      texts: item.segments.map((segment) => segment.text),
    })));
    return batch.map((item) => ({
      chunkIndex: item.chunk.index,
      startSec: item.chunk.startSec,
      endSec: item.chunk.endSec,
      summary: `Batch ${item.chunk.index}`,
      topics: [`Batch ${item.chunk.index}`],
      decisions: [],
      actions: [],
      questions: [],
      risks: [],
    }));
  },
  updateChunkFacts: async (chunk, status, factsJson, errorMsg) => {
    batchUpdates.push({ index: chunk.index, status, factsJson, errorMsg });
  },
});

assert.equal(batchCalls.length, 1);
assert.deepEqual(batchCalls[0].map((item) => item.index), [1, 2]);
assert.deepEqual(batchCalls[0][0].texts, ["acao"]);
assert.deepEqual(batched.map((item) => item.summary), ["Batch 1", "Batch 2"]);
assert.deepEqual(
  batchUpdates.map((item) => [item.index, item.status]),
  [
    [1, "done"],
    [2, "done"],
  ],
);

const failureUpdates = [];
await assert.rejects(
  extractMeetingFactsConcurrently({
    chunks: [chunks[1]],
    apiKey: "gemini-key",
    concurrency: 1,
    parseSegments: (chunk) => JSON.parse(chunk.rawSegmentsJson),
    extractChunkFacts: async () => {
      throw new Error("facts failed");
    },
    updateChunkFacts: async (chunk, status, factsJson, errorMsg) => {
      failureUpdates.push({ index: chunk.index, status, factsJson, errorMsg });
    },
  }),
  /facts failed/,
);

assert.deepEqual(
  failureUpdates.map((item) => [item.index, item.status, item.errorMsg ?? ""]),
  [
    [1, "running", ""],
    [1, "error", "facts failed"],
  ],
);

let runningUpdateResolved = false;
let extractorStartedBeforeRunningPersisted = false;
await extractMeetingFactsConcurrently({
  chunks: [chunks[1]],
  apiKey: "gemini-key",
  concurrency: 1,
  parseSegments: (chunk) => JSON.parse(chunk.rawSegmentsJson),
  extractChunkFacts: async (chunk, segments) => {
    assert.equal(chunk.index, 1);
    assert.equal(segments.length, 1);
    extractorStartedBeforeRunningPersisted = !runningUpdateResolved;
    return {
      chunkIndex: chunk.index,
      startSec: chunk.startSec,
      endSec: chunk.endSec,
      summary: "Nao aguardou status running",
      topics: [],
      decisions: [],
      actions: [],
      questions: [],
      risks: [],
    };
  },
  updateChunkFacts: async (_chunk, status) => {
    if (status === "running") {
      await new Promise((resolve) => setTimeout(resolve, 30));
      runningUpdateResolved = true;
    }
  },
});

assert.equal(extractorStartedBeforeRunningPersisted, true);
