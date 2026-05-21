import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { rmSync, mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";

const repo = process.cwd();
const outDir = join(process.env.TEMP, "meeting-minutes-processing-concurrency-test");
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
      "src/lib/processingConcurrency.ts",
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
  factConcurrencyForPhase,
  factConcurrencyForProfile,
  transcriptionConcurrencyForProfile,
} = require(join(outDir, "processingConcurrency.js"));

assert.equal(transcriptionConcurrencyForProfile("turbo"), 6);
assert.equal(transcriptionConcurrencyForProfile("balanced"), 4);
assert.equal(transcriptionConcurrencyForProfile("precision"), 3);

assert.equal(factConcurrencyForProfile("turbo"), 4);
assert.equal(factConcurrencyForProfile("balanced"), 3);
assert.equal(factConcurrencyForProfile("precision"), 3);

assert.equal(factConcurrencyForPhase("turbo", false), 2);
assert.equal(factConcurrencyForPhase("balanced", false), 1);
assert.equal(factConcurrencyForPhase("precision", false), 1);
assert.equal(factConcurrencyForPhase("turbo", true), 4);
assert.equal(factConcurrencyForPhase("balanced", true), 3);
assert.equal(factConcurrencyForPhase("precision", true), 3);
