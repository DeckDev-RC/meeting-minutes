import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { rmSync, mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";

const repo = process.cwd();
const outDir = join(process.env.TEMP, "meeting-minutes-executive-test");
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
      "src/lib/executiveMinutes.ts",
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

const { buildExecutiveMinutesHtml, selectExecutiveActions } = require(
  join(outDir, "executiveMinutes.js"),
);

const baseStructuredMinutes = {
  minuteId: "minute-1",
  meetingId: "meeting-1",
  htmlContent: "<div>Completa</div>",
  pdfPath: null,
  modelUsed: "test",
  userEdited: false,
  participantNames: ["Caio", "Naiara"],
  createdAt: "2026-05-25T10:00:00Z",
  decisions: [
    {
      id: "decision-1",
      minuteId: "minute-1",
      meetingId: "meeting-1",
      itemIndex: 0,
      chunkIndex: 0,
      title: "Definir regra de despesa fixa versus despesa extra",
      owner: "Caio",
      timestampSec: 1554,
      evidence: "vamos alinhar o que e padrao fixo",
      evidenceId: "ev-1",
      createdAt: "2026-05-25T10:00:00Z",
    },
  ],
  actions: [
    {
      id: "action-ui",
      minuteId: "minute-1",
      meetingId: "meeting-1",
      itemIndex: 0,
      chunkIndex: 0,
      task: "Clicar e arrastar para o lado",
      owner: "Marcelo",
      deadline: "A definir",
      timestampSec: 1200,
      evidence: "clica clica, Marcelo, arrasta para o lado",
      evidenceId: "ev-2",
      status: "pending",
      priority: "normal",
      completedAt: null,
      createdAt: "2026-05-25T10:00:00Z",
    },
    {
      id: "action-business",
      minuteId: "minute-1",
      meetingId: "meeting-1",
      itemIndex: 1,
      chunkIndex: 0,
      task: "Ajustar a contabilidade de janeiro a abril, criando um plano de conta separado para recebimentos especificos de clientes",
      owner: "Naiara",
      deadline: "A definir",
      timestampSec: 1449,
      evidence: "ajustar com a Naiara para tras, janeiro ate abril",
      evidenceId: "ev-3",
      status: "pending",
      priority: "normal",
      completedAt: null,
      createdAt: "2026-05-25T10:00:00Z",
    },
  ],
  evidences: [],
  versions: [],
};

const selectedActions = selectExecutiveActions(baseStructuredMinutes.actions, { limit: 8 });

assert.equal(selectedActions.length, 1);
assert.equal(selectedActions[0].id, "action-business");

const executiveHtml = buildExecutiveMinutesHtml(baseStructuredMinutes, {
  title: "Ata Executiva",
});

assert.match(executiveHtml, /Ata Executiva/);
assert.match(executiveHtml, /Ajustar a contabilidade de janeiro a abril/);
assert.doesNotMatch(executiveHtml, /Clicar e arrastar/);
assert.doesNotMatch(executiveHtml, /Rastreabilidade/);
