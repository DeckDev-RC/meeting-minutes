import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { rmSync, mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";

const repo = process.cwd();
const outDir = join(process.env.TEMP, "meeting-minutes-transcription-provider-test");
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
      "src/lib/transcriptionProvider.ts",
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
  isLocalTranscriptionBackend,
  LOCAL_TRANSCRIPTION_REQUIRED_AUDIO_SEC,
  selectTranscriptionBackend,
  transcriptionBackendLabel,
} = require(join(outDir, "transcriptionProvider.js"));

assert.equal(LOCAL_TRANSCRIPTION_REQUIRED_AUDIO_SEC, 7200);

assert.equal(
  selectTranscriptionBackend({
    totalAudioSec: 4 * 3600 + 46 * 60,
    groqApiKey: "gsk_live",
    profile: "groq-turbo",
  }),
  "groq",
);

assert.equal(
  selectTranscriptionBackend({
    totalAudioSec: 4 * 3600 + 46 * 60,
    cloudflareAccountId: "account",
    cloudflareApiToken: "token",
    groqApiKey: "gsk_live",
    profile: "smart-low-cost",
  }),
  "cloudflare",
);

assert.equal(
  selectTranscriptionBackend({
    totalAudioSec: 1800,
    cloudflareAccountId: "account",
    cloudflareApiToken: "token",
    deepgramApiKey: "dg_live",
    profile: "max-quality",
  }),
  "deepgram",
);

assert.equal(
  selectTranscriptionBackend({
    totalAudioSec: 1800,
    groqApiKey: "gsk_live",
    profile: "manual",
    manualProvider: "groq",
  }),
  "groq",
);

assert.equal(
  selectTranscriptionBackend({
    totalAudioSec: 1800,
    profile: "offline-free",
  }),
  "parakeet-local",
);

assert.equal(
  selectTranscriptionBackend({
    totalAudioSec: 1800,
    groqApiKey: "",
  }),
  "local",
);

assert.equal(
  selectTranscriptionBackend({
    totalAudioSec: 1800,
    groqApiKey: "gsk_live",
  }),
  "groq",
);

assert.equal(isLocalTranscriptionBackend("parakeet-local"), true);
assert.equal(isLocalTranscriptionBackend("local"), true);
assert.equal(isLocalTranscriptionBackend("groq"), false);
assert.equal(isLocalTranscriptionBackend("cloudflare"), false);
assert.equal(isLocalTranscriptionBackend("deepgram"), false);
assert.equal(transcriptionBackendLabel("parakeet-local"), "Parakeet local");
assert.equal(transcriptionBackendLabel("local"), "faster-whisper local");
assert.equal(transcriptionBackendLabel("groq"), "Groq Whisper");
assert.equal(transcriptionBackendLabel("cloudflare"), "Cloudflare Whisper");
assert.equal(transcriptionBackendLabel("deepgram"), "Deepgram Nova-3");
