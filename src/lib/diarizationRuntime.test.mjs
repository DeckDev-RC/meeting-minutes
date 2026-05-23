import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { rmSync, mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";

const repo = process.cwd();
const outDir = join(process.env.TEMP, "meeting-minutes-diarization-runtime-test");
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
      "src/lib/diarizationRuntime.ts",
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
  normalizeSpeakerDiarizationRuntime,
  chunkOutputFormatForSpeakerRuntime,
  sherpaProviderForRuntime,
  shouldTrySherpaRuntime,
} = require(join(outDir, "diarizationRuntime.js"));

assert.equal(normalizeSpeakerDiarizationRuntime("sherpa-onnx-cpu"), "sherpa-onnx-cpu");
assert.equal(normalizeSpeakerDiarizationRuntime("sherpa-onnx-cuda"), "sherpa-onnx-cuda");
assert.equal(normalizeSpeakerDiarizationRuntime("unknown"), "modern-cpu");

assert.equal(sherpaProviderForRuntime("sherpa-onnx-cpu"), "cpu");
assert.equal(sherpaProviderForRuntime("sherpa-onnx-cuda"), "cuda");
assert.equal(sherpaProviderForRuntime("modern-cpu"), undefined);

assert.equal(shouldTrySherpaRuntime("sherpa-onnx-cuda", 4), true);
assert.equal(shouldTrySherpaRuntime("sherpa-onnx-cuda", 1), false);
assert.equal(shouldTrySherpaRuntime("modern-cpu", 4), false);

assert.equal(chunkOutputFormatForSpeakerRuntime("modern-cpu"), "flac");
assert.equal(chunkOutputFormatForSpeakerRuntime("sherpa-onnx-cpu"), "wav");
assert.equal(chunkOutputFormatForSpeakerRuntime("sherpa-onnx-cuda"), "wav");
