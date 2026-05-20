import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const repo = process.cwd();
const tmpDir = join(process.env.TEMP, "meeting-minutes-evaluation-cli-test");
const manifestPath = join(tmpDir, "manifest.json");
const runPath = join(tmpDir, "run.json");

rmSync(tmpDir, { recursive: true, force: true });
mkdirSync(tmpDir, { recursive: true });

writeFileSync(
  manifestPath,
  JSON.stringify({
    version: 1,
    thresholds: {
      minThroughputX: 2,
      maxRealTimeFactor: 0.5,
      minFactRecall: 0.5,
      minFactPrecision: 0.5,
    },
    cases: [
      {
        id: "case-1",
        title: "Sample case",
        dataset: "MeetingBank",
        language: "en",
        durationSec: 600,
        referenceItems: [
          {
            id: "decision-1",
            kind: "decision",
            text: "Approve the plan",
            requiredTerms: ["approve", "plan"],
          },
        ],
      },
    ],
  }),
);

writeFileSync(
  runPath,
  JSON.stringify({
    version: 1,
    cases: [
      {
        caseId: "case-1",
        processingSec: 120,
        output: {
          decisions: [
            {
              title: "Approved the plan",
              owner: "",
              timestampSec: 10,
              evidence: "Plan approved.",
            },
          ],
        },
      },
    ],
  }),
);

const jsonRun = spawnSync(
  "node",
  ["scripts/evaluateMeetingRun.mjs", "--manifest", manifestPath, "--run", runPath, "--format", "json"],
  { cwd: repo, encoding: "utf8" },
);

assert.equal(jsonRun.status, 0, jsonRun.stderr);
const parsed = JSON.parse(jsonRun.stdout);
assert.equal(parsed.status, "pass");
assert.equal(parsed.aggregate.speed.throughputX, 5);
assert.equal(parsed.aggregate.factScore.recall, 1);

const markdownRun = spawnSync(
  "node",
  ["scripts/evaluateMeetingRun.mjs", "--manifest", manifestPath, "--run", runPath, "--format", "markdown"],
  { cwd: repo, encoding: "utf8" },
);

assert.equal(markdownRun.status, 0, markdownRun.stderr);
assert.ok(markdownRun.stdout.includes("Benchmark report: PASS"));
assert.ok(markdownRun.stdout.includes("case-1"));
assert.ok(markdownRun.stdout.includes("5.00x"));

const usageRun = spawnSync("node", ["scripts/evaluateMeetingRun.mjs"], {
  cwd: repo,
  encoding: "utf8",
});

assert.equal(usageRun.status, 1);
assert.ok(usageRun.stderr.includes("Usage:"));
