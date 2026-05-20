import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { rmSync, mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";

const repo = process.cwd();
const outDir = join(process.env.TEMP, "meeting-minutes-segment-merge-test");
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
      "src/lib/segmentMerge.ts",
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

const { mergeSortedTranscriptionSegments } = require(join(outDir, "segmentMerge.js"));

const completed = [
  { id: 70, start: 0, end: 1, text: "completed-0" },
  { id: 71, start: 20, end: 21, text: "completed-20" },
  { id: 72, start: 50, end: 51, text: "completed-50" },
];
const created = [
  { id: 80, start: 10, end: 11, text: "new-10" },
  { id: 81, start: 20, end: 21, text: "new-20-same-start" },
  { id: 82, start: 60, end: 61, text: "new-60" },
];

const merged = mergeSortedTranscriptionSegments(completed, created);

assert.deepEqual(
  merged.map((segment) => segment.text),
  [
    "completed-0",
    "new-10",
    "completed-20",
    "new-20-same-start",
    "completed-50",
    "new-60",
  ],
);
assert.deepEqual(merged.map((segment) => segment.id), [0, 1, 2, 3, 4, 5]);

assert.deepEqual(mergeSortedTranscriptionSegments([], created).map((segment) => segment.id), [
  0,
  1,
  2,
]);
