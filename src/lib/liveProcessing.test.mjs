import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { rmSync, mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";

const repo = process.cwd();
const outDir = join(process.env.TEMP, "meeting-minutes-live-processing-test");
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
      "src/lib/liveProcessing.ts",
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
  applyLiveTranscriptSpeakers,
  appendLiveInsights,
  appendLiveLog,
  appendLiveTranscript,
  buildLiveMinutesDraft,
  createLiveProcessingState,
  formatLiveTimestamp,
} = require(join(outDir, "liveProcessing.js"));

assert.equal(formatLiveTimestamp(64.2), "01:04");
assert.equal(formatLiveTimestamp(3723.7), "01:02:03");

let state = createLiveProcessingState();
state = appendLiveTranscript(
  state,
  1,
  [
    { id: 0, start: 68, end: 72, text: "Segundo trecho" },
    { id: 1, start: 62, end: 66, text: "Primeiro trecho" },
  ],
  5,
);

assert.deepEqual(
  state.transcript.map((item) => [item.chunkIndex, item.timeLabel, item.endTimeLabel, item.text, item.segmentCount]),
  [
    [1, "01:02", "01:12", "Primeiro trecho Segundo trecho", 2],
  ],
);

state = appendLiveTranscript(
  state,
  2,
  Array.from({ length: 8 }, (_, index) => ({
    id: index,
    start: 100 + index * 5,
    end: 101 + index * 5,
    text: `Linha ${index}`,
  })),
  5,
);

assert.equal(state.transcript.length, 5);
assert.deepEqual(
  state.transcript.map((item) => item.text),
  ["Linha 3", "Linha 4", "Linha 5", "Linha 6", "Linha 7"],
);

state = applyLiveTranscriptSpeakers(state, [
  { speaker: "Caio", start: 115, end: 117, text: "Linha 3" },
  { speaker: "Emanuella", start: 120, end: 126, text: "Linha 4" },
]);

assert.deepEqual(
  state.transcript.slice(0, 2).map((item) => item.speaker),
  ["Caio", "Emanuella"],
);

state = appendLiveInsights(state, {
  chunkIndex: 2,
  startSec: 100,
  endSec: 120,
  summary: "Resumo objetivo do trecho.",
  topics: ["Integração", "Relatório"],
  decisions: [
    {
      title: "Usar planilha como relatório final",
      owner: "Caio",
      timestampSec: 110,
      evidence: "relatório precisa ser planilha",
    },
  ],
  actions: [
    {
      task: "Validar processamento de PDF",
      owner: "Emanuella",
      deadline: "sexta",
      timestampSec: 112,
      evidence: "testar PDFs de cliente",
    },
  ],
  questions: ["Quem atualiza o drive?"],
  risks: ["Drivers desatualizados"],
});

assert.equal(state.insights.length, 1);
assert.equal(state.insights[0].decisionCount, 1);
assert.equal(state.insights[0].actionCount, 1);
assert.equal(state.minutesDraft, "");

const draft = buildLiveMinutesDraft(state.insights, ["Caio", "Emanuella"]);
assert.match(draft, /Participantes: Caio, Emanuella/);
assert.match(draft, /Usar planilha como relatório final/);
assert.match(draft, /Validar processamento de PDF/);

state = appendLiveLog(state, "info", "Transcrição iniciada", 10);
state = appendLiveLog(state, "success", "Chunk 2 concluído", 12);
assert.deepEqual(
  state.logs.map((item) => [item.level, item.timeLabel, item.message]),
  [
    ["info", "00:10", "Transcrição iniciada"],
    ["success", "00:12", "Chunk 2 concluído"],
  ],
);
