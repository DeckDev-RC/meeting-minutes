import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { rmSync, mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";

const repo = process.cwd();
const outDir = join(process.env.TEMP, "meeting-minutes-speaker-count-test");
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
      "src/lib/speakerCount.ts",
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
  inferExpectedSpeakersFromParticipants,
  resolveDiarizationExpectedSpeakers,
  shouldPreferChunkedDiarization,
} = require(join(outDir, "speakerCount.js"));

assert.equal(inferExpectedSpeakersFromParticipants(["Caio", "Emanuella"]), 2);
assert.equal(inferExpectedSpeakersFromParticipants(["Caio", " caio ", "Emanuella"]), 2);
assert.equal(inferExpectedSpeakersFromParticipants(["Caio"]), undefined);
assert.equal(
  inferExpectedSpeakersFromParticipants([
    "P1",
    "P2",
    "P3",
    "P4",
    "P5",
    "P6",
    "P7",
    "P8",
    "P9",
  ]),
  undefined,
);

assert.equal(resolveDiarizationExpectedSpeakers(3, ["Caio", "Emanuella"]), 3);
assert.equal(resolveDiarizationExpectedSpeakers(undefined, ["Caio", "Emanuella"]), 2);
assert.equal(resolveDiarizationExpectedSpeakers(1, ["Caio", "Emanuella"]), 2);

assert.equal(shouldPreferChunkedDiarization(2, 3), true);
assert.equal(shouldPreferChunkedDiarization(undefined, 3), false);
assert.equal(shouldPreferChunkedDiarization(2, 1), false);
