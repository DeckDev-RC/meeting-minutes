#!/usr/bin/env node
import { createRequire } from "node:module";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);

function usage() {
  return [
    "Usage: node scripts/evaluateMeetingRun.mjs --manifest <manifest.json> --run <run.json> [--format json|markdown]",
    "",
    "The command exits with code 2 when benchmark gates fail.",
  ].join("\n");
}

function parseArgs(argv) {
  const args = {
    manifest: "",
    run: "",
    format: "markdown",
  };

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    const next = argv[index + 1];

    if (arg === "--manifest" && next) {
      args.manifest = next;
      index += 1;
    } else if (arg === "--run" && next) {
      args.run = next;
      index += 1;
    } else if (arg === "--format" && next) {
      args.format = next;
      index += 1;
    } else if (arg === "--help" || arg === "-h") {
      args.help = true;
    } else {
      throw new Error(`Unknown or incomplete argument: ${arg}`);
    }
  }

  return args;
}

function loadEvaluationModule() {
  const scriptDir = dirname(fileURLToPath(import.meta.url));
  const repoRoot = resolve(scriptDir, "..");
  const sourcePath = resolve(repoRoot, "src/lib/evaluation.ts");
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
  const localRequire = (id) => {
    if (id === "./types") {
      return {};
    }

    return require(id);
  };

  new Function("require", "exports", "module", compiled)(localRequire, module.exports, module);
  return module.exports;
}

function readJson(path) {
  return JSON.parse(readFileSync(resolve(path), "utf8"));
}

try {
  const args = parseArgs(process.argv.slice(2));
  if (args.help) {
    console.log(usage());
    process.exit(0);
  }

  if (!args.manifest || !args.run) {
    console.error(usage());
    process.exit(1);
  }

  if (!["json", "markdown"].includes(args.format)) {
    throw new Error(`Unsupported format: ${args.format}`);
  }

  const { evaluateBenchmarkRun, renderBenchmarkReportMarkdown } = loadEvaluationModule();
  const report = evaluateBenchmarkRun(readJson(args.manifest), readJson(args.run));

  if (args.format === "json") {
    console.log(JSON.stringify(report, null, 2));
  } else {
    process.stdout.write(renderBenchmarkReportMarkdown(report));
  }

  process.exitCode = report.status === "pass" ? 0 : 2;
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}
