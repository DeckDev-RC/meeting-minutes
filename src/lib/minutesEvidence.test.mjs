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
  purgeUnverifiedMeetingInsightsEvidence,
  sanitizeMeetingChunkInsights,
  summarizeEvidencePurge,
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
  topicEvidence: [
    {
      title: "  Drive  ",
      timestampSec: 126,
      evidence: "Caio continua responsavel pelo Drive",
    },
    {
      title: "",
      timestampSec: 126,
      evidence: "Sem titulo",
    },
  ],
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
assert.deepEqual(sanitized.topicEvidence, [
  {
    title: "Drive",
    timestampSec: 126,
    evidence: "Caio continua responsavel pelo Drive",
  },
]);
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

assert.equal(validation.total, 3);
assert.equal(validation.verified, 3);
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
    topicEvidence: undefined,
    actions: [],
  },
  [{ id: 1, start: 120, end: 130, text: "O assunto era outro." }],
);

assert.equal(weak.total, 1);
assert.equal(weak.verified, 0);
assert.equal(weak.items[0].verified, false);

const purged = purgeUnverifiedMeetingInsightsEvidence(
  {
    chunkIndex: 0,
    startSec: 0,
    endSec: 60,
    summary: "Resumo",
    topics: ["Entrega final", "Fornecedor externo"],
    topicEvidence: [
      {
        title: "Entrega final",
        timestampSec: 4,
        evidence: "Caio aprovou a entrega final",
      },
      {
        title: "Fornecedor externo",
        timestampSec: 20,
        evidence: "fornecedor externo foi aprovado por todos",
      },
    ],
    decisions: [
      {
        title: "Aprovar entrega",
        owner: "Caio",
        timestampSec: 4,
        evidence: "Caio aprovou a entrega final",
      },
      {
        title: "Cortar escopo",
        owner: "Equipe",
        timestampSec: 8,
        evidence: "essa frase nunca apareceu na transcricao",
      },
    ],
    actions: [
      {
        task: "Enviar resumo",
        owner: "Maria",
        deadline: "hoje",
        timestampSec: 12,
        evidence: "Maria envia o resumo revisado hoje",
      },
      {
        task: "Contratar fornecedor",
        owner: "Financeiro",
        deadline: "sexta",
        timestampSec: 20,
        evidence: "fornecedor externo foi aprovado por todos",
      },
    ],
    questions: [],
    risks: [],
  },
  [
    { id: 1, start: 0, end: 5, text: "Caio aprovou a entrega final." },
    { id: 2, start: 8, end: 14, text: "Maria envia o resumo revisado hoje." },
  ],
);

assert.deepEqual(purged.insights.topics, ["Entrega final"]);
assert.deepEqual(purged.insights.topicEvidence.map((topic) => topic.title), ["Entrega final"]);
assert.deepEqual(
  purged.insights.decisions.map((decision) => decision.title),
  ["Aprovar entrega"],
);
assert.deepEqual(
  purged.insights.actions.map((action) => action.task),
  ["Enviar resumo"],
);
assert.deepEqual(
  purged.removed.map((item) => `${item.kind}:${item.label}`),
  ["topic:Fornecedor externo", "decision:Cortar escopo", "action:Contratar fornecedor"],
);
assert.deepEqual(summarizeEvidencePurge(purged.removed), {
  removedTopics: 1,
  removedDecisions: 1,
  removedActions: 1,
  removedTotal: 3,
});

const purgedTopicWithoutEvidence = purgeUnverifiedMeetingInsightsEvidence(
  {
    chunkIndex: 1,
    startSec: 60,
    endSec: 90,
    summary: "Resumo",
    topics: ["Tema sem lastro"],
    topicEvidence: [],
    decisions: [],
    actions: [],
    questions: [],
    risks: [],
  },
  [{ id: 3, start: 60, end: 70, text: "Nada sobre esse tema foi falado." }],
);

assert.deepEqual(purgedTopicWithoutEvidence.insights.topics, []);
assert.deepEqual(purgedTopicWithoutEvidence.removed.map((item) => `${item.kind}:${item.label}`), [
  "topic:Tema sem lastro",
]);
