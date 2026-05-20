#!/usr/bin/env node
import { createRequire } from "node:module";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const DATASETS = {
  meetingbank: {
    dataset: "huuuyeah/meetingbank",
    config: "default",
    split: "test",
  },
  publichearingbr: {
    dataset: "unicamp-dl/PublicHearingBR",
    config: "default",
    split: "train",
  },
};

function usage() {
  return [
    "Usage: node scripts/runTextBenchmark.mjs --dataset meetingbank|publichearingbr [options]",
    "",
    "Options:",
    "  --rows-file <path>              Use a local Hugging Face rows JSON fixture instead of downloading.",
    "  --hf-base-url <url>             Dataset Viewer base URL. Default: https://datasets-server.huggingface.co",
    "  --out-dir <path>                Output directory. Default: benchmarks/runs/text-<timestamp>",
    "  --offset <n>                    Hugging Face row offset. Default: 0",
    "  --length <n>                    Hugging Face rows length. Default: 1",
    "  --max-cases <n>                 Max cases to process. Default: length",
    "  --transcript-char-limit <n>     Limit transcript chars per case. Default: 12000",
    "  --target-chars <n>              Chunk target chars. Default: 6000",
    "  --concurrency <n>               Gemini chunk extraction concurrency. Default: 1",
    "  --model <name>                  Gemini model. Default: gemini-2.5-flash",
    "  --api-key-env <name>            API key environment variable. Default: GEMINI_API_KEY",
    "  --dry-run                       Build sources and draft manifest without calling Gemini.",
  ].join("\n");
}

function parseArgs(argv) {
  const args = {
    dataset: "",
    rowsFile: "",
    hfBaseUrl: "https://datasets-server.huggingface.co",
    outDir: "",
    offset: 0,
    length: 1,
    maxCases: undefined,
    transcriptCharLimit: 12000,
    targetChars: 6000,
    concurrency: 1,
    model: "gemini-2.5-flash",
    apiKeyEnv: "GEMINI_API_KEY",
    dryRun: false,
  };

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    const next = argv[index + 1];

    if (arg === "--dataset" && next) {
      args.dataset = next;
      index += 1;
    } else if (arg === "--rows-file" && next) {
      args.rowsFile = next;
      index += 1;
    } else if (arg === "--hf-base-url" && next) {
      args.hfBaseUrl = next.replace(/\/+$/, "");
      index += 1;
    } else if (arg === "--out-dir" && next) {
      args.outDir = next;
      index += 1;
    } else if (arg === "--offset" && next) {
      args.offset = parseInteger(next, "offset");
      index += 1;
    } else if (arg === "--length" && next) {
      args.length = parseInteger(next, "length");
      index += 1;
    } else if (arg === "--max-cases" && next) {
      args.maxCases = parseInteger(next, "max-cases");
      index += 1;
    } else if (arg === "--transcript-char-limit" && next) {
      args.transcriptCharLimit = parseInteger(next, "transcript-char-limit");
      index += 1;
    } else if (arg === "--target-chars" && next) {
      args.targetChars = parseInteger(next, "target-chars");
      index += 1;
    } else if (arg === "--concurrency" && next) {
      args.concurrency = parseInteger(next, "concurrency");
      index += 1;
    } else if (arg === "--model" && next) {
      args.model = next;
      index += 1;
    } else if (arg === "--api-key-env" && next) {
      args.apiKeyEnv = next;
      index += 1;
    } else if (arg === "--dry-run") {
      args.dryRun = true;
    } else if (arg === "--help" || arg === "-h") {
      args.help = true;
    } else {
      throw new Error(`Unknown or incomplete argument: ${arg}`);
    }
  }

  return args;
}

function parseInteger(value, name) {
  const parsed = Number.parseInt(value, 10);
  if (!Number.isFinite(parsed) || parsed < 0) {
    throw new Error(`Invalid ${name}: ${value}`);
  }

  return parsed;
}

function loadTsModule(relativePath) {
  const scriptDir = dirname(fileURLToPath(import.meta.url));
  const repoRoot = resolve(scriptDir, "..");
  const sourcePath = resolve(repoRoot, relativePath);
  const source = readFileSync(sourcePath, "utf8");
  const ts = require("typescript");
  const compiled = ts.transpileModule(source, {
    compilerOptions: {
      target: ts.ScriptTarget.ES2021,
      module: ts.ModuleKind.CommonJS,
      esModuleInterop: true,
    },
  }).outputText;

  const module = { exports: {} };
  new Function("require", "exports", "module", compiled)(require, module.exports, module);
  return module.exports;
}

async function loadRows(args) {
  if (args.rowsFile) {
    return JSON.parse(readFileSync(resolve(args.rowsFile), "utf8"));
  }

  const config = DATASETS[args.dataset];
  const params = new URLSearchParams({
    dataset: config.dataset,
    config: config.config,
    split: config.split,
    offset: String(args.offset),
    length: String(args.length),
  });
  const url = `${args.hfBaseUrl}/rows?${params}`;
  const response = await fetch(url);
  if (response.ok) {
    return response.json();
  }

  const fallbackParams = new URLSearchParams({
    dataset: config.dataset,
    config: config.config,
    split: config.split,
  });
  const fallbackUrl = `${args.hfBaseUrl}/first-rows?${fallbackParams}`;
  const fallbackResponse = await fetch(fallbackUrl);
  if (fallbackResponse.ok) {
    return fallbackResponse.json();
  }

  throw new Error(
    `Failed to fetch Hugging Face rows: /rows HTTP ${response.status}; /first-rows HTTP ${fallbackResponse.status}`,
  );
}

async function callGemini({ apiKey, model, prompt, responseMimeType, maxOutputTokens }) {
  const response = await fetch(
    `https://generativelanguage.googleapis.com/v1beta/models/${encodeURIComponent(model)}:generateContent`,
    {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "x-goog-api-key": apiKey,
      },
      body: JSON.stringify({
        contents: [
          {
            role: "user",
            parts: [{ text: prompt }],
          },
        ],
        generationConfig: {
          temperature: 0.1,
          maxOutputTokens,
          ...(responseMimeType ? { responseMimeType } : {}),
        },
      }),
    },
  );

  if (!response.ok) {
    const body = await response.text();
    throw new Error(`Gemini request failed: HTTP ${response.status} ${body.slice(0, 500)}`);
  }

  const json = await response.json();
  const text = json?.candidates?.[0]?.content?.parts
    ?.map((part) => part.text ?? "")
    .join("")
    .trim();
  if (!text) {
    throw new Error("Gemini returned an empty response");
  }

  return text;
}

async function mapLimit(items, limit, task) {
  const results = new Array(items.length);
  let nextIndex = 0;

  async function worker() {
    while (nextIndex < items.length) {
      const index = nextIndex;
      nextIndex += 1;
      results[index] = await task(items[index], index);
    }
  }

  await Promise.all(
    Array.from({ length: Math.max(1, Math.min(limit, items.length)) }, () => worker()),
  );
  return results;
}

function writeJson(path, value) {
  writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`);
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  if (args.help) {
    console.log(usage());
    return 0;
  }

  if (!args.dataset || !DATASETS[args.dataset]) {
    console.error(usage());
    return 1;
  }

  const {
    buildDraftManifestFromTextCases,
    buildGeminiChunkFactsPrompt,
    buildGeminiFinalMinutesPrompt,
    chunkTextBenchmarkCase,
    extractTextBenchmarkCases,
    parseGeminiChunkFactsText,
  } = loadTsModule("src/lib/textBenchmark.ts");
  const { buildBenchmarkRun } = loadTsModule("src/lib/benchmarkRun.ts");
  const { evaluateBenchmarkRun, renderBenchmarkReportMarkdown } = loadTsModule("src/lib/evaluation.ts");

  const outDir =
    args.outDir ||
    resolve(
      "benchmarks",
      "runs",
      `text-${new Date().toISOString().replace(/[:.]/g, "-")}`,
    );
  mkdirSync(outDir, { recursive: true });

  const rows = await loadRows(args);
  const cases = extractTextBenchmarkCases(args.dataset, rows, {
    maxCases: args.maxCases ?? args.length,
    transcriptCharLimit: args.transcriptCharLimit,
  });

  if (cases.length === 0) {
    throw new Error("No benchmark cases were extracted from the dataset rows");
  }

  const sourcesPath = resolve(outDir, "text-benchmark-sources.json");
  const manifestPath = resolve(outDir, "text-benchmark-manifest-draft.json");
  writeJson(sourcesPath, cases);
  const manifest = buildDraftManifestFromTextCases(cases, {
    minThroughputX: 3,
    maxRealTimeFactor: 0.35,
  });
  manifest.cases = manifest.cases.map((item) => ({
    ...item,
    referenceItems: [],
  }));
  writeJson(manifestPath, manifest);

  if (args.dryRun) {
    console.log(`Dry run complete. Sources: ${sourcesPath}`);
    console.log(`Draft manifest: ${manifestPath}`);
    return 0;
  }

  const apiKey = process.env[args.apiKeyEnv]?.trim();
  if (!apiKey) {
    throw new Error(`${args.apiKeyEnv} is required unless --dry-run is used`);
  }

  const runCases = [];
  for (const benchmarkCase of cases) {
    const startedAt = Date.now();
    const chunks = chunkTextBenchmarkCase(benchmarkCase, { targetChars: args.targetChars });
    console.log(`Processing ${benchmarkCase.id}: ${chunks.length} text chunks`);
    const facts = await mapLimit(chunks, args.concurrency, async (chunk) => {
      const prompt = buildGeminiChunkFactsPrompt(chunk, benchmarkCase);
      const text = await callGemini({
        apiKey,
        model: args.model,
        prompt,
        responseMimeType: "application/json",
        maxOutputTokens: 8192,
      });
      return parseGeminiChunkFactsText(text, chunk);
    });
    const finalHtml = await callGemini({
      apiKey,
      model: args.model,
      prompt: buildGeminiFinalMinutesPrompt(benchmarkCase, facts),
      maxOutputTokens: 8192,
    });
    writeFileSync(resolve(outDir, `${benchmarkCase.id}-minutes.html`), finalHtml);

    const processingSec = (Date.now() - startedAt) / 1000;
    const run = buildBenchmarkRun({
      meetingId: benchmarkCase.id,
      title: benchmarkCase.title,
      sourcePath: benchmarkCase.sourceRef,
      processingSec,
      audioSec: benchmarkCase.estimatedDurationSec,
      engine: `text-benchmark-${args.model}`,
      speakers: [],
      facts,
    });
    runCases.push(run.cases[0]);
  }

  const benchmarkRun = {
    version: 1,
    engine: `text-benchmark-${args.model}`,
    createdAt: new Date().toISOString(),
    cases: runCases,
  };
  const runPath = resolve(outDir, "text-benchmark-run.json");
  writeJson(runPath, benchmarkRun);

  const report = evaluateBenchmarkRun(manifest, benchmarkRun);
  const reportPath = resolve(outDir, "text-benchmark-report.md");
  writeFileSync(reportPath, renderBenchmarkReportMarkdown(report));

  console.log(`Run: ${runPath}`);
  console.log(`Report: ${reportPath}`);
  return report.status === "pass" ? 0 : 2;
}

main()
  .then((code) => {
    process.exitCode = code;
  })
  .catch((error) => {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  });
