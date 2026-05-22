import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { rmSync, mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";

const repo = process.cwd();
const outDir = join(process.env.TEMP, "meeting-minutes-transcription-quality-test");
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
      "src/lib/transcriptionQuality.ts",
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

const { scoreCloudflareTranscriptRisk } = require(join(outDir, "transcriptionQuality.js"));

assert.equal(
  scoreCloudflareTranscriptRisk({
    durationSec: 120,
    segments: [{ id: 0, start: 0, end: 1, text: "Meu nome e Emanuela." }],
  }).shouldEscalate,
  true,
);

assert.equal(
  scoreCloudflareTranscriptRisk({
    durationSec: 10,
    segments: [{ id: 0, start: 0, end: 10, text: "Caio falou com Manuela sobre Drive." }],
  }).shouldEscalate,
  false,
);

assert.equal(
  scoreCloudflareTranscriptRisk({
    durationSec: 300,
    segments: [{ id: 0, start: 0, end: 5, text: "texto curto" }],
  }).shouldEscalate,
  true,
);
