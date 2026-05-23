import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { rmSync, mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";

const repo = process.cwd();
const outDir = join(process.env.TEMP, "meeting-minutes-evidence-test");
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
      "src/lib/minutesEvidence.ts",
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
  evidenceSimilarity,
  sanitizeMeetingChunkInsights,
  validateMeetingInsightsEvidence,
} = require(join(outDir, "minutesEvidence.js"));

assert.equal(
  evidenceSimilarity("Caio definiu o prazo de sexta-feira.", "caio definiu prazo sexta feira"),
  1,
);
assert.ok(
  evidenceSimilarity(
    "A Emanuella comentou que os arquivos do Drive nao processaram.",
    "Emanuela comentou arquivos drive nao foram processados"
  ) >= 0.58,
);
assert.ok(evidenceSimilarity("texto sem relacao", "prazo caio sexta") < 0.4);

const rawInsight = {
  chunkIndex: 2,
  startSec: 120,
  endSec: 180,
  summary: "Resumo valido",
  topics: ["  Drive  ", "", 42],
  decisions: [
    {
      title: "Manter Caio como responsavel",
      owner: "Caio",
      timestampSec: 125,
      evidence: "Caio continua responsavel pelo Drive",
    },
    {
      title: "",
      owner: "Caio",
      timestampSec: 125,
      evidence: "sem titulo",
    },
    {
      title: "Fora do chunk",
      owner: "Caio",
      timestampSec: 999,
      evidence: "fora",
    },
  ],
  actions: [
    {
      task: "Revisar relatorio",
      owner: "Emanuella",
      deadline: "sexta",
      timestampSec: 160,
      evidence: "Emanuella vai revisar o relatorio ate sexta",
    },
    {
      task: "Sem evidencia",
      owner: "Emanuella",
      deadline: "",
      timestampSec: 160,
      evidence: "",
    },
  ],
  questions: ["  Qual o status? ", "", 10],
  risks: ["PDFs nao processados", null],
};

const sanitized = sanitizeMeetingChunkInsights(rawInsight);
assert.equal(sanitized.chunkIndex, 2);
assert.deepEqual(sanitized.topics, ["Drive"]);
assert.deepEqual(sanitized.questions, ["Qual o status?"]);
assert.deepEqual(sanitized.risks, ["PDFs nao processados"]);
assert.equal(sanitized.decisions.length, 1);
assert.equal(sanitized.actions.length, 1);

const validation = validateMeetingInsightsEvidence(sanitized, [
  {
    id: 1,
    start: 123,
    end: 130,
    text: "O Caio continua responsavel pelo Drive e vai acompanhar isso.",
  },
  {
    id: 2,
    start: 158,
    end: 165,
    text: "A Emanuella vai revisar o relatorio ate sexta com o time.",
  },
]);

assert.equal(validation.total, 2);
assert.equal(validation.verified, 2);
assert.equal(validation.items.every((item) => item.verified), true);

const weak = validateMeetingInsightsEvidence(
  {
    ...sanitized,
    decisions: [
      {
        title: "Outra decisao",
        owner: "Caio",
        timestampSec: 125,
        evidence: "Uma frase inventada que nao aparece na transcricao",
      },
    ],
    actions: [],
  },
  [{ id: 1, start: 120, end: 130, text: "O assunto era outro." }],
);

assert.equal(weak.total, 1);
assert.equal(weak.verified, 0);
assert.equal(weak.items[0].verified, false);
