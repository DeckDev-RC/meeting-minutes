import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { rmSync, mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";

const repo = process.cwd();
const outDir = join(process.env.TEMP, "meeting-minutes-transcription-preflight-test");
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
      "src/lib/transcriptionPreflight.ts",
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
  buildTranscriptionPreflight,
  estimateDeepgramCost,
  mapBudgetToTranscriptionProfile,
} = require(join(outDir, "transcriptionPreflight.js"));

assert.equal(mapBudgetToTranscriptionProfile("free-local"), "offline-free");
assert.equal(mapBudgetToTranscriptionProfile("low-cost"), "smart-low-cost");
assert.equal(mapBudgetToTranscriptionProfile("max-quality"), "max-quality");

assert.equal(estimateDeepgramCost(3 * 3600).currency, "USD");
assert.equal(estimateDeepgramCost(3 * 3600).amount.toFixed(2), "1.28");

const lowCost = buildTranscriptionPreflight({
  durationSec: 3 * 3600,
  budgetProfile: "low-cost",
  groqApiKey: "groq",
  cloudflareAccountId: "account",
  cloudflareApiToken: "token",
  deepgramApiKey: "deepgram",
  cloudflareQuotaExhaustedToday: false,
});

assert.equal(lowCost.backend, "cloudflare");
assert.deepEqual(lowCost.fallbackBackends, ["deepgram", "groq", "local"]);
assert.equal(lowCost.quotaRisk.level, "high");
assert.equal(lowCost.deepgramFallbackCost.amount.toFixed(2), "1.28");

const exhausted = buildTranscriptionPreflight({
  durationSec: 3 * 3600,
  budgetProfile: "low-cost",
  groqApiKey: "groq",
  cloudflareAccountId: "account",
  cloudflareApiToken: "token",
  deepgramApiKey: "deepgram",
  cloudflareQuotaExhaustedToday: true,
});

assert.equal(exhausted.backend, "deepgram");
assert.equal(exhausted.quotaRisk.level, "blocked");
assert.equal(exhausted.fallbackBackends.includes("local"), true);

const free = buildTranscriptionPreflight({
  durationSec: 3 * 3600,
  budgetProfile: "free-local",
  cloudflareAccountId: "account",
  cloudflareApiToken: "token",
  deepgramApiKey: "deepgram",
  cloudflareQuotaExhaustedToday: true,
});

assert.equal(free.backend, "parakeet-local");
assert.deepEqual(free.fallbackBackends, []);
assert.equal(free.deepgramFallbackCost.amount, 0);
