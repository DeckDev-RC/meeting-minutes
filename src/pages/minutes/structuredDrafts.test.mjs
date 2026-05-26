import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { rmSync, mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";

const repo = process.cwd();
const outDir = join(process.env.TEMP, "meeting-minutes-structured-drafts-test");
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

const {
  createDecisionDraftState,
  createActionDraftState,
  mergeDecisionDraftState,
  mergeActionDraftState,
} = require(join(outDir, "pages", "minutes", "structuredDrafts.js"));

const decision = {
  id: "decision-1",
  title: "Aprovar contrato",
  owner: "Caio",
  timestampSec: 12,
  evidence: "Contrato aprovado na reuniao",
};

const initialDecisionState = createDecisionDraftState([decision]);
const dirtyDecisionState = {
  ...initialDecisionState,
  values: {
    ...initialDecisionState.values,
    "decision-1": {
      ...initialDecisionState.values["decision-1"],
      title: "Aprovar contrato revisado",
    },
  },
};

const recreatedDecision = {
  ...decision,
  owner: "Maria",
};

const mergedDecisionState = mergeDecisionDraftState([recreatedDecision], dirtyDecisionState);

assert.equal(
  mergedDecisionState.values["decision-1"].title,
  "Aprovar contrato revisado",
  "dirty decision title should survive parent array recreation",
);
assert.equal(
  mergedDecisionState.values["decision-1"].owner,
  "Maria",
  "clean decision owner should sync from new server data",
);
assert.deepEqual(Object.keys(mergedDecisionState.values), ["decision-1"]);

const action = {
  id: "action-1",
  task: "Enviar resumo",
  owner: "Maria",
  deadline: "sexta",
  timestampSec: 33,
  evidence: "Maria combinou o envio",
  status: "pending",
  priority: "normal",
  completedAt: null,
};

const initialActionState = createActionDraftState([action]);
const dirtyActionState = {
  bases: {
    ...initialActionState.bases,
    "removed-action": initialActionState.bases["action-1"],
  },
  values: {
    ...initialActionState.values,
    "action-1": {
      ...initialActionState.values["action-1"],
      deadline: "segunda",
      status: "in_progress",
    },
    "removed-action": {
      ...initialActionState.values["action-1"],
      task: "Nao deve sobreviver",
    },
  },
};

const recreatedAction = {
  ...action,
  owner: "Rafaela",
  priority: "high",
};

const mergedActionState = mergeActionDraftState([recreatedAction], dirtyActionState);

assert.equal(
  mergedActionState.values["action-1"].deadline,
  "segunda",
  "dirty action deadline should survive parent array recreation",
);
assert.equal(
  mergedActionState.values["action-1"].status,
  "in_progress",
  "dirty action status should survive parent array recreation",
);
assert.equal(
  mergedActionState.values["action-1"].owner,
  "Rafaela",
  "clean action owner should sync from new server data",
);
assert.equal(
  mergedActionState.values["action-1"].priority,
  "high",
  "clean action priority should sync from new server data",
);
assert.deepEqual(Object.keys(mergedActionState.values), ["action-1"]);
