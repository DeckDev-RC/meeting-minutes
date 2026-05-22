import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { rmSync, mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";

const repo = process.cwd();
const outDir = join(process.env.TEMP, "meeting-minutes-cloud-health-test");
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
      "src/lib/cloudTranscriptionHealth.ts",
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
  clearCloudflareQuotaExhausted,
  getCloudflareQuotaState,
  markCloudflareQuotaExhausted,
} = require(join(outDir, "cloudTranscriptionHealth.js"));

class MemoryStorage {
  values = new Map();

  getItem(key) {
    return this.values.get(key) ?? null;
  }

  setItem(key, value) {
    this.values.set(key, String(value));
  }

  removeItem(key) {
    this.values.delete(key);
  }
}

const storage = new MemoryStorage();
const may22 = new Date(2026, 4, 22, 16, 0, 0);
const may23 = new Date(2026, 4, 23, 9, 0, 0);

assert.equal(getCloudflareQuotaState(storage, may22).isExhaustedToday, false);

markCloudflareQuotaExhausted(storage, may22, "429 daily free allocation");
const exhausted = getCloudflareQuotaState(storage, may22);
assert.equal(exhausted.isExhaustedToday, true);
assert.equal(exhausted.reason, "429 daily free allocation");

assert.equal(getCloudflareQuotaState(storage, may23).isExhaustedToday, false);

clearCloudflareQuotaExhausted(storage);
assert.equal(getCloudflareQuotaState(storage, may22).isExhaustedToday, false);
