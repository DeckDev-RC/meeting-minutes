import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { rmSync, mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";

const repo = process.cwd();
const outDir = join(process.env.TEMP, "meeting-minutes-live-processing-cache-test");
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
      "src/lib/liveProcessingCache.ts",
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

const { collectExpiredLiveProcessingSnapshotIds, collectOverflowLiveProcessingSnapshotIds } = require(
  join(outDir, "liveProcessingCache.js"),
);

const expired = collectExpiredLiveProcessingSnapshotIds({
  completedAtByMeetingId: new Map([
    ["old-done", 1_000],
    ["recent-done", 9_500],
    ["running", 500],
    ["visible", 1_000],
  ]),
  activeMeetingIds: new Set(["running"]),
  visibleMeetingId: "visible",
  nowMs: 10_000,
  ttlMs: 5_000,
});

assert.deepEqual(expired, ["old-done"]);

const overflow = collectOverflowLiveProcessingSnapshotIds({
  snapshotIds: ["oldest", "visible", "running", "middle", "newest"],
  activeMeetingIds: new Set(["running"]),
  visibleMeetingId: "visible",
  touchedAtByMeetingId: new Map([
    ["oldest", 1_000],
    ["middle", 2_000],
    ["newest", 3_000],
    ["visible", 500],
    ["running", 400],
  ]),
  maxEntries: 3,
});

assert.deepEqual(overflow, ["oldest", "middle"]);
