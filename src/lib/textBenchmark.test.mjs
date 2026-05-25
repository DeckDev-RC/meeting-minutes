import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { rmSync, mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";

const repo = process.cwd();
const outDir = join(process.env.TEMP, "meeting-minutes-text-benchmark-test");
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
      "src/lib/textBenchmark.ts",
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
  buildDraftManifestFromTextCases,
  buildGeminiChunkFactsPrompt,
  buildGeminiFinalMinutesPrompt,
  chunkTextBenchmarkCase,
  extractTextBenchmarkCases,
  parseGeminiChunkFactsText,
} = require(join(outDir, "textBenchmark.js"));

const meetingBankResponse = {
  rows: [
    {
      row_idx: 7,
      row: {
        uid: "Seattle_001",
        summary: "The council approved the plan and requested a report.",
        transcript:
          "Chair opened the meeting. The council approved the plan. Ana will send a report next week. The finance team reviewed the budget. Members asked about implementation dates. The chair requested written follow up. Staff confirmed that procurement can begin after legal review. The meeting adjourned.",
      },
    },
  ],
};

const meetingBankCases = extractTextBenchmarkCases("meetingbank", meetingBankResponse, {
  maxCases: 1,
  transcriptCharLimit: 1000,
});

assert.equal(meetingBankCases.length, 1);
assert.equal(meetingBankCases[0].id, "meetingbank-Seattle_001");
assert.equal(meetingBankCases[0].dataset, "MeetingBank");
assert.equal(meetingBankCases[0].language, "en");
assert.equal(meetingBankCases[0].title, "Seattle_001");
assert.ok(meetingBankCases[0].estimatedDurationSec > 0);
assert.equal(meetingBankCases[0].referenceSummary, meetingBankResponse.rows[0].row.summary);

const publicHearingResponse = {
  rows: [
    {
      row_idx: 2,
      row: {
        id: 99,
        materia: "Deputados discutem custos de energia\nSubtitulo ignorado",
        metadados: {
          assunto: "Custos de energia",
        },
        transcricao:
          "O SR. PRESIDENTE - Declaro aberta a reuniao. A convidada Maria informou os custos. O deputado pediu dados adicionais.",
      },
    },
  ],
};

const publicHearingCases = extractTextBenchmarkCases("publichearingbr", publicHearingResponse, {
  maxCases: 1,
  transcriptCharLimit: 1000,
});

assert.equal(publicHearingCases[0].id, "publichearingbr-99");
assert.equal(publicHearingCases[0].dataset, "PublicHearingBR");
assert.equal(publicHearingCases[0].language, "pt-BR");
assert.equal(publicHearingCases[0].title, "Custos de energia");
assert.equal(publicHearingCases[0].referenceSummary, "Deputados discutem custos de energia\nSubtitulo ignorado");

const chunks = chunkTextBenchmarkCase(meetingBankCases[0], { targetChars: 200 });
assert.ok(chunks.length > 1);
assert.equal(chunks[0].chunkIndex, 0);
assert.equal(chunks.at(-1).endSec, meetingBankCases[0].estimatedDurationSec);
assert.equal(
  chunks.map((chunk) => chunk.text).join(" ").replace(/\s+/g, " ").trim(),
  meetingBankCases[0].transcript,
);
assert.ok(chunks.every((chunk) => chunk.endSec > chunk.startSec));

const prompt = buildGeminiChunkFactsPrompt(chunks[0], meetingBankCases[0]);
assert.ok(prompt.includes("Return only valid JSON"));
assert.ok(prompt.includes('"chunkIndex"'));
assert.ok(prompt.includes('"topicEvidence"'));
assert.ok(prompt.includes(chunks[0].text));

const finalPrompt = buildGeminiFinalMinutesPrompt(meetingBankCases[0], [
  {
    chunkIndex: 0,
    startSec: 0,
    endSec: 10,
    summary: "Approved the plan.",
    topics: ["Plan"],
    decisions: [],
    actions: [],
    questions: [],
    risks: [],
  },
]);
assert.ok(finalPrompt.includes("HTML"));
assert.ok(finalPrompt.includes("Approved the plan."));

const parsed = parseGeminiChunkFactsText(
  "```json\n{\"summary\":\"Resumo\",\"topics\":[\"Topico\"],\"topicEvidence\":[{\"title\":\"Topico\",\"timestampSec\":2,\"evidence\":\"Topico discutido\"}],\"decisions\":[],\"actions\":[],\"questions\":[],\"risks\":[]}\n```",
  chunks[0],
);

assert.equal(parsed.chunkIndex, 0);
assert.equal(parsed.startSec, chunks[0].startSec);
assert.equal(parsed.summary, "Resumo");
assert.deepEqual(parsed.topics, ["Topico"]);
assert.deepEqual(parsed.topicEvidence, [
  {
    title: "Topico",
    timestampSec: 2,
    evidence: "Topico discutido",
  },
]);

const fallback = parseGeminiChunkFactsText("not json", chunks[0]);
assert.equal(fallback.chunkIndex, chunks[0].chunkIndex);
assert.ok(fallback.summary.length > 0);
assert.deepEqual(fallback.decisions, []);

const manifest = buildDraftManifestFromTextCases(meetingBankCases, {
  minThroughputX: 3,
  maxRealTimeFactor: 0.35,
});

assert.equal(manifest.version, 1);
assert.equal(manifest.cases[0].id, "meetingbank-Seattle_001");
assert.equal(manifest.cases[0].referenceItems[0].kind, "summary");
assert.ok(manifest.cases[0].referenceItems[0].requiredTerms.length >= 3);
