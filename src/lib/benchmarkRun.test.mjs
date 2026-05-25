import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { rmSync, mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";

const repo = process.cwd();
const outDir = join(process.env.TEMP, "meeting-minutes-benchmark-run-test");
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
      "src/lib/benchmarkRun.ts",
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
  buildBenchmarkRun,
  buildBenchmarkRunArtifactPath,
  buildBenchmarkRunFileName,
  buildBenchmarkRunPath,
} = require(join(outDir, "benchmarkRun.js"));

const run = buildBenchmarkRun({
  meetingId: "meeting-123",
  title: "Reuniao produto",
  sourcePath: "C:\\Meetings\\produto.mp3",
  processingSec: 125.4,
  audioSec: 600,
  engine: "meeting-minutes-local",
  speakers: ["SPEAKER_00", "SPEAKER_01"],
  purgeSummary: {
    removedTopics: 1,
    removedDecisions: 2,
    removedActions: 1,
    removedTotal: 4,
  },
  facts: [
    {
      chunkIndex: 0,
      startSec: 0,
      endSec: 300,
      summary: "Primeira metade",
      topics: ["Roadmap", "Entrega"],
      decisions: [
        {
          title: "Aprovar roadmap",
          owner: "Ana",
          timestampSec: 42,
          evidence: "Vamos aprovar o roadmap.",
        },
      ],
      actions: [
        {
          task: "Bruno envia proposta",
          owner: "Bruno",
          deadline: "sexta",
          timestampSec: 99,
          evidence: "Bruno ficou de enviar.",
        },
      ],
      questions: ["Quem valida a planilha?"],
      risks: ["Prazo apertado"],
    },
    {
      chunkIndex: 1,
      startSec: 300,
      endSec: 600,
      summary: "Segunda metade",
      topics: ["Roadmap", "Financeiro"],
      decisions: [],
      actions: [],
      questions: ["Quem valida a planilha?"],
      risks: [],
    },
  ],
});

assert.equal(run.version, 1);
assert.equal(run.engine, "meeting-minutes-local");
assert.equal(run.cases.length, 1);
assert.equal(run.cases[0].caseId, "meeting-123");
assert.equal(run.cases[0].processingSec, 125.4);
assert.equal(run.cases[0].audioSec, 600);
assert.deepEqual(run.cases[0].output.speakers, ["SPEAKER_00", "SPEAKER_01"]);
assert.deepEqual(run.cases[0].output.topics, ["Roadmap", "Entrega", "Financeiro"]);
assert.equal(run.cases[0].output.summary, "Primeira metade\n\nSegunda metade");
assert.equal(run.cases[0].output.decisions.length, 1);
assert.equal(run.cases[0].output.actions.length, 1);
assert.deepEqual(run.cases[0].output.questions, ["Quem valida a planilha?"]);
assert.deepEqual(run.cases[0].output.risks, ["Prazo apertado"]);
assert.equal(run.cases[0].metadata.title, "Reuniao produto");
assert.equal(run.cases[0].metadata.sourcePath, "C:\\Meetings\\produto.mp3");
assert.deepEqual(run.cases[0].metadata.purgeSummary, {
  removedTopics: 1,
  removedDecisions: 2,
  removedActions: 1,
  removedTotal: 4,
});

assert.equal(
  buildBenchmarkRunFileName("meeting-123"),
  "meeting-123_benchmark-run.json",
);
assert.equal(
  buildBenchmarkRunPath("C:\\Meetings\\produto.mp3", "meeting-123"),
  "C:\\Meetings\\meeting-123_benchmark-run.json",
);
assert.equal(
  buildBenchmarkRunPath("/tmp/produto.mp3", "meeting-123"),
  "/tmp/meeting-123_benchmark-run.json",
);
assert.equal(
  buildBenchmarkRunArtifactPath(
    "C:\\Users\\User\\AppData\\Roaming\\com.agregar.meeting-minutes\\processing\\meeting-123\\",
    "meeting-123",
  ),
  "C:\\Users\\User\\AppData\\Roaming\\com.agregar.meeting-minutes\\processing\\meeting-123\\meeting-123_benchmark-run.json",
);
