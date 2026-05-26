import assert from "node:assert/strict";
import { performance } from "node:perf_hooks";
import { spawnSync } from "node:child_process";
import { mkdirSync, rmSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";

const repo = process.cwd();
const outDir = join(process.env.TEMP, "meeting-minutes-hardening-benchmark");
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
      "src/pages/minutes/structuredDrafts.ts",
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

const { createActionDraftState, mergeActionDraftState } = require(
  join(outDir, "pages", "minutes", "structuredDrafts.js"),
);

const actions = Array.from({ length: 5000 }, (_, index) => ({
  id: `action-${index}`,
  task: `Task ${index}`,
  owner: index % 2 === 0 ? "Caio" : "Maria",
  deadline: "sexta",
  timestampSec: index,
  evidence: `Evidence ${index}`,
  status: "pending",
  priority: "normal",
  completedAt: null,
}));

const initial = createActionDraftState(actions);
const dirty = {
  ...initial,
  values: {
    ...initial.values,
    "action-4999": {
      ...initial.values["action-4999"],
      task: "Dirty final task",
    },
  },
};

const updated = actions.map((action) => ({
  ...action,
  owner: "Rafaela",
}));

const startedAt = performance.now();
const merged = mergeActionDraftState(updated, dirty);
const elapsedMs = performance.now() - startedAt;

assert.equal(merged.values["action-4999"].task, "Dirty final task");
assert.equal(merged.values["action-1"].owner, "Rafaela");

console.log(
  JSON.stringify(
    {
      benchmark: "structured-draft-merge",
      items: actions.length,
      elapsedMs: Number(elapsedMs.toFixed(3)),
      opsPerSecond: Number((actions.length / (elapsedMs / 1000)).toFixed(0)),
    },
    null,
    2,
  ),
);
