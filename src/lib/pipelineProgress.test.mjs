import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { rmSync, mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";

const repo = process.cwd();
const outDir = join(process.env.TEMP, "meeting-minutes-pipeline-progress-test");
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
      "src/lib/pipelineProgress.ts",
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

const { derivePipelineProgress } = require(join(outDir, "pipelineProgress.js"));

assert.deepEqual(
  derivePipelineProgress({
    phase: "transcribe",
    completedAudioSec: 900,
    totalAudioSec: 1800,
    completedChunks: 3,
    totalChunks: 6,
    elapsedMs: 60_000,
  }),
  {
    percent: 50,
    title: "Transcrevendo em paralelo",
    detail: "3 de 6 blocos transcritos. 15m00s de 30m00s de audio processados.",
    etaLabel: "1m00s restantes",
    speedLabel: "15.0x tempo real",
  }
);

assert.deepEqual(
  derivePipelineProgress({
    phase: "transcribe",
    completedAudioSec: 0,
    totalAudioSec: 1800,
    completedChunks: 0,
    totalChunks: 6,
    elapsedMs: 60_000,
  }),
  {
    percent: 18,
    title: "Transcrevendo em paralelo",
    detail: "0 de 6 blocos transcritos. 0m00s de 30m00s de audio processados.",
    etaLabel: "Calculando tempo restante",
    speedLabel: "Calculando velocidade",
  }
);

const invalidView = derivePipelineProgress({
  phase: "transcribe",
  completedAudioSec: Number.NaN,
  totalAudioSec: Number.POSITIVE_INFINITY,
  completedChunks: Number.POSITIVE_INFINITY,
  totalChunks: Number.NaN,
  elapsedMs: 0,
});

assert.equal(Number.isFinite(invalidView.percent), true);
assert.equal(invalidView.percent, 18);
assert.equal(invalidView.title, "Transcrevendo em paralelo");
assert.equal(invalidView.detail, "0 de 0 blocos transcritos. 0m00s de 0m00s de audio processados.");
assert.equal(invalidView.etaLabel, "Calculando tempo restante");
assert.equal(invalidView.speedLabel, "Calculando velocidade");
assert.equal(/NaN|Infinity/.test(JSON.stringify(invalidView)), false);

assert.deepEqual(
  derivePipelineProgress({
    phase: "detect_speech",
    completedAudioSec: 0,
    totalAudioSec: 0,
    completedChunks: -2,
    totalChunks: -1,
    elapsedMs: 500,
  }),
  {
    percent: 10,
    title: "Detectando fala e pausas",
    detail: "Analisando pausas para encontrar bons pontos de corte no audio.",
    etaLabel: "",
    speedLabel: "",
  }
);

assert.deepEqual(
  derivePipelineProgress({
    phase: "diarize",
    completedAudioSec: 993,
    totalAudioSec: 993,
    completedChunks: 3,
    totalChunks: 3,
    elapsedMs: 7249,
  }),
  {
    percent: 88,
    title: "Identificando falantes em paralelo",
    detail: "Motor local trabalhando em paralelo. 3 de 3 blocos ja tem transcricao disponivel.",
    etaLabel: "",
    speedLabel: "",
  }
);

assert.deepEqual(
  derivePipelineProgress({
    phase: "extract_facts",
    completedAudioSec: 993,
    totalAudioSec: 993,
    completedChunks: 2,
    totalChunks: 3,
    elapsedMs: 7249,
  }),
  {
    percent: 93,
    title: "Extraindo decisoes e acoes",
    detail: "Lendo 2 de 3 blocos para separar decisoes, tarefas, riscos e perguntas.",
    etaLabel: "2 de 3 blocos de insights",
    speedLabel: "67% dos insights",
  }
);

assert.deepEqual(
  derivePipelineProgress({
    phase: "extract_facts",
    completedAudioSec: 993,
    totalAudioSec: 993,
    completedChunks: 3,
    totalChunks: 3,
    elapsedMs: 7249,
  }),
  {
    percent: 95,
    title: "Aguardando falantes",
    detail: "3 de 3 blocos de insights prontos. Falantes ainda em processamento antes da ata.",
    etaLabel: "Insights completos",
    speedLabel: "Tempo total 0m07s",
  }
);

assert.deepEqual(
  derivePipelineProgress({
    phase: "wait_speakers",
    completedAudioSec: 993,
    totalAudioSec: 993,
    completedChunks: 3,
    totalChunks: 3,
    elapsedMs: 160_000,
  }),
  {
    percent: 96,
    title: "Aguardando falantes",
    detail: "Insights prontos. Mantendo a ata em espera ate a identificacao de falantes terminar.",
    etaLabel: "Falantes em andamento",
    speedLabel: "Tempo total 2m40s",
  }
);

assert.deepEqual(
  derivePipelineProgress({
    phase: "complete",
    completedAudioSec: Number.POSITIVE_INFINITY,
    totalAudioSec: Number.POSITIVE_INFINITY,
    completedChunks: 99,
    totalChunks: 3,
    elapsedMs: 1,
  }),
  {
    percent: 100,
    title: "Processamento concluido",
    detail: "Ata gerada e salva. Abrindo a visualizacao final.",
    etaLabel: "Concluido",
    speedLabel: "",
  }
);
