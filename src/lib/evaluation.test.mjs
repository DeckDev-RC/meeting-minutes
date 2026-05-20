import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { rmSync, mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";

const repo = process.cwd();
const outDir = join(process.env.TEMP, "meeting-minutes-evaluation-test");
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
      "src/lib/evaluation.ts",
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
  evaluateBenchmarkRun,
  normalizeEvaluationText,
  renderBenchmarkReportMarkdown,
} = require(join(outDir, "evaluation.js"));

assert.equal(normalizeEvaluationText("Ação do João: Enviar proposta!"), "acao do joao enviar proposta");

const manifest = {
  version: 1,
  thresholds: {
    minThroughputX: 3,
    maxRealTimeFactor: 0.35,
    minFactRecall: 0.6,
    minFactPrecision: 0.6,
  },
  cases: [
    {
      id: "meetingbank-long-001",
      title: "Long public meeting sample",
      dataset: "MeetingBank",
      language: "en",
      durationSec: 1200,
      expectedSpeakers: 4,
      referenceItems: [
        {
          id: "decision-1",
          kind: "decision",
          text: "The team approved the delivery schedule.",
          requiredTerms: ["approved", "delivery schedule"],
        },
        {
          id: "action-1",
          kind: "action",
          text: "Ana must send the proposal.",
          requiredTerms: ["ana", "send", "proposal"],
        },
        {
          id: "question-1",
          kind: "question",
          text: "Who will validate the final spreadsheet?",
          requiredTerms: ["validate", "spreadsheet"],
        },
      ],
    },
  ],
};

const run = {
  version: 1,
  engine: "local-dev",
  cases: [
    {
      caseId: "meetingbank-long-001",
      processingSec: 240,
      output: {
        speakers: ["SPEAKER_00", "SPEAKER_01", "SPEAKER_02"],
        summary: "The delivery schedule was approved and Ana will send the proposal.",
        topics: ["Schedule"],
        decisions: [
          {
            title: "Approved delivery schedule",
            owner: "",
            timestampSec: 30,
            evidence: "The delivery schedule was approved.",
          },
        ],
        actions: [
          {
            task: "Ana will send the proposal",
            owner: "Ana",
            deadline: "",
            timestampSec: 60,
            evidence: "Ana agreed to send the proposal.",
          },
          {
            task: "Buy snacks",
            owner: "Bruno",
            deadline: "",
            timestampSec: 90,
            evidence: "Snacks were mentioned.",
          },
        ],
        questions: [],
        risks: [],
      },
    },
  ],
};

const report = evaluateBenchmarkRun(manifest, run);

assert.equal(report.status, "pass");
assert.equal(report.cases.length, 1);
assert.equal(report.cases[0].caseId, "meetingbank-long-001");
assert.equal(report.cases[0].speed.audioSec, 1200);
assert.equal(report.cases[0].speed.processingSec, 240);
assert.equal(report.cases[0].speed.throughputX, 5);
assert.equal(report.cases[0].speed.realTimeFactor, 0.2);
assert.equal(report.cases[0].speakerDelta, -1);

assert.deepEqual(report.cases[0].factScore.byKind.decision, {
  referenceCount: 1,
  predictionCount: 1,
  matchedCount: 1,
  precision: 1,
  recall: 1,
  f1: 1,
});
assert.deepEqual(report.cases[0].factScore.byKind.action, {
  referenceCount: 1,
  predictionCount: 2,
  matchedCount: 1,
  precision: 0.5,
  recall: 1,
  f1: 0.667,
});
assert.deepEqual(report.cases[0].factScore.byKind.question, {
  referenceCount: 1,
  predictionCount: 0,
  matchedCount: 0,
  precision: 0,
  recall: 0,
  f1: 0,
});
assert.equal(report.cases[0].factScore.overall.referenceCount, 3);
assert.equal(report.cases[0].factScore.overall.predictionCount, 3);
assert.equal(report.cases[0].factScore.overall.matchedCount, 2);
assert.equal(report.cases[0].factScore.overall.precision, 0.667);
assert.equal(report.cases[0].factScore.overall.recall, 0.667);
assert.equal(report.cases[0].factScore.overall.f1, 0.667);
assert.deepEqual(report.cases[0].missedReferenceIds, ["question-1"]);
assert.equal(report.cases[0].unmatchedPredictionCount, 1);

const failingReport = evaluateBenchmarkRun(
  {
    ...manifest,
    thresholds: {
      minThroughputX: 8,
      maxRealTimeFactor: 0.1,
      minFactRecall: 0.9,
      minFactPrecision: 0.9,
    },
  },
  run,
);

assert.equal(failingReport.status, "fail");
assert.ok(failingReport.gateFailures.includes("aggregate throughput 5.00x < 8.00x"));
assert.ok(failingReport.gateFailures.includes("aggregate RTF 0.200 > 0.100"));
assert.ok(failingReport.gateFailures.includes("aggregate fact recall 66.7% < 90.0%"));
assert.ok(failingReport.gateFailures.includes("aggregate fact precision 66.7% < 90.0%"));

const markdown = renderBenchmarkReportMarkdown(report);
assert.ok(markdown.includes("meetingbank-long-001"));
assert.ok(markdown.includes("5.00x"));
assert.ok(markdown.includes("66.7%"));

const speedOnlyReport = evaluateBenchmarkRun(
  {
    version: 1,
    thresholds: { minThroughputX: 3 },
    cases: [
      {
        id: "speed-only",
        title: "Speed only",
        dataset: "MeetingBank",
        language: "en",
        durationSec: 60,
        referenceItems: [],
      },
    ],
  },
  {
    version: 1,
    cases: [
      {
        caseId: "speed-only",
        processingSec: 10,
        output: {
          summary: "A summary without a manual reference.",
          decisions: [],
          actions: [],
        },
      },
    ],
  },
);

const speedOnlyMarkdown = renderBenchmarkReportMarkdown(speedOnlyReport);
assert.ok(speedOnlyMarkdown.includes("Aggregate facts: n/a"));
assert.ok(speedOnlyMarkdown.includes("| speed-only | MeetingBank | 6.00x | n/a | n/a | n/a | 0 | 0 |"));
