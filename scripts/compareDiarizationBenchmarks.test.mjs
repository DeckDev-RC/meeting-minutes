import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { compareReports } from "./compareDiarizationBenchmarks.mjs";

const repo = process.cwd();
const tmpDir = join(process.env.TEMP, "meeting-minutes-diarization-compare-test");
const baselinePath = join(tmpDir, "baseline.json");
const candidatePath = join(tmpDir, "candidate.json");
const slowCandidatePath = join(tmpDir, "slow-candidate.json");

rmSync(tmpDir, { recursive: true, force: true });
mkdirSync(tmpDir, { recursive: true });

const baseline = {
  mode: "baseline",
  wallClockSec: 200,
  realtimeFactor: 0.2,
  speedX: 5,
  projectedThreeHourMin: 36,
  speakerCount: 4,
  diarizedSegmentCount: 90,
};
const candidate = {
  mode: "candidate",
  wallClockSec: 100,
  realtimeFactor: 0.1,
  speedX: 10,
  projectedThreeHourMin: 18,
  speakerCount: 4,
  diarizedSegmentCount: 95,
};
const slowCandidate = {
  ...candidate,
  wallClockSec: 210,
  realtimeFactor: 0.21,
  speedX: 4.76,
  projectedThreeHourMin: 37.8,
};

writeFileSync(baselinePath, JSON.stringify(baseline));
writeFileSync(candidatePath, JSON.stringify(candidate));
writeFileSync(slowCandidatePath, JSON.stringify(slowCandidate));

const comparison = compareReports(baseline, candidate, {
  minSpeedup: 1.5,
  maxRtf: 0.15,
  expectedSpeakers: 4,
});
assert.equal(comparison.status, "pass");
assert.equal(comparison.metrics.speedup, 2);
assert.equal(comparison.metrics.projectedThreeHourDeltaMin, -18);

const failing = compareReports(baseline, slowCandidate, { minSpeedup: 1.1 });
assert.equal(failing.status, "fail");
assert.ok(failing.failures[0].includes("speedup"));

const cliPass = spawnSync(
  "node",
  [
    "scripts/compareDiarizationBenchmarks.mjs",
    "--baseline",
    baselinePath,
    "--candidate",
    candidatePath,
    "--min-speedup",
    "1.5",
    "--max-rtf",
    "0.15",
    "--expected-speakers",
    "4",
  ],
  { cwd: repo, encoding: "utf8" },
);
assert.equal(cliPass.status, 0, cliPass.stderr);
assert.ok(cliPass.stdout.includes("PASS"));
assert.ok(cliPass.stdout.includes("2.000x"));

const cliFail = spawnSync(
  "node",
  [
    "scripts/compareDiarizationBenchmarks.mjs",
    "--baseline",
    baselinePath,
    "--candidate",
    slowCandidatePath,
    "--min-speedup",
    "1.1",
  ],
  { cwd: repo, encoding: "utf8" },
);
assert.equal(cliFail.status, 2);
assert.ok(cliFail.stdout.includes("FAIL"));
