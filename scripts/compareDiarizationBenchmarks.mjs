#!/usr/bin/env node
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

function usage() {
  return [
    "Usage: node scripts/compareDiarizationBenchmarks.mjs --baseline <report.json> --candidate <report.json> [options]",
    "",
    "Options:",
    "  --min-speedup <n>       Required candidate speedup vs baseline. Default: 1",
    "  --max-rtf <n>           Maximum candidate realtime factor. Default: no gate",
    "  --expected-speakers <n> Fail if candidate speaker count differs. Default: no gate",
    "  --format json|markdown  Output format. Default: markdown",
  ].join("\n");
}

function parseArgs(argv) {
  const args = {
    baseline: "",
    candidate: "",
    minSpeedup: 1,
    maxRtf: undefined,
    expectedSpeakers: undefined,
    format: "markdown",
  };

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    const next = argv[index + 1];

    if (arg === "--baseline" && next) {
      args.baseline = next;
      index += 1;
    } else if (arg === "--candidate" && next) {
      args.candidate = next;
      index += 1;
    } else if (arg === "--min-speedup" && next) {
      args.minSpeedup = parsePositiveNumber(next, "min-speedup");
      index += 1;
    } else if (arg === "--max-rtf" && next) {
      args.maxRtf = parsePositiveNumber(next, "max-rtf");
      index += 1;
    } else if (arg === "--expected-speakers" && next) {
      args.expectedSpeakers = parsePositiveInteger(next, "expected-speakers");
      index += 1;
    } else if (arg === "--format" && next) {
      if (next !== "json" && next !== "markdown") {
        throw new Error(`Invalid format: ${next}`);
      }
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

function parsePositiveNumber(value, name) {
  const parsed = Number(value);
  if (!Number.isFinite(parsed) || parsed <= 0) {
    throw new Error(`Invalid ${name}: ${value}`);
  }
  return parsed;
}

function parsePositiveInteger(value, name) {
  const parsed = Number.parseInt(value, 10);
  if (!Number.isFinite(parsed) || parsed <= 0) {
    throw new Error(`Invalid ${name}: ${value}`);
  }
  return parsed;
}

function readReport(path) {
  const report = JSON.parse(readFileSync(path, "utf8"));
  for (const field of ["wallClockSec", "realtimeFactor", "speedX"]) {
    if (!Number.isFinite(Number(report[field]))) {
      throw new Error(`Report ${path} is missing numeric ${field}`);
    }
  }
  return {
    path,
    mode: String(report.mode || "unknown"),
    wallClockSec: Number(report.wallClockSec),
    realtimeFactor: Number(report.realtimeFactor),
    speedX: Number(report.speedX),
    projectedThreeHourMin: Number(report.projectedThreeHourMin || 0),
    speakerCount: Number(report.speakerCount || 0),
    diarizedSegmentCount: Number(report.diarizedSegmentCount || 0),
  };
}

function round(value, digits = 2) {
  const factor = 10 ** digits;
  return Math.round(value * factor) / factor;
}

export function compareReports(baseline, candidate, options = {}) {
  const speedup = baseline.wallClockSec / candidate.wallClockSec;
  const speedXDelta = candidate.speedX - baseline.speedX;
  const wallDeltaSec = candidate.wallClockSec - baseline.wallClockSec;
  const failures = [];
  const minSpeedup = options.minSpeedup ?? 1;

  if (speedup < minSpeedup) {
    failures.push(
      `speedup ${round(speedup, 3)}x is below required ${round(minSpeedup, 3)}x`,
    );
  }

  if (options.maxRtf !== undefined && candidate.realtimeFactor > options.maxRtf) {
    failures.push(
      `candidate RTF ${round(candidate.realtimeFactor, 3)} is above max ${round(options.maxRtf, 3)}`,
    );
  }

  if (
    options.expectedSpeakers !== undefined &&
    candidate.speakerCount !== options.expectedSpeakers
  ) {
    failures.push(
      `candidate speaker count ${candidate.speakerCount} differs from expected ${options.expectedSpeakers}`,
    );
  }

  return {
    status: failures.length === 0 ? "pass" : "fail",
    failures,
    baseline,
    candidate,
    metrics: {
      speedup: round(speedup, 3),
      wallDeltaSec: round(wallDeltaSec, 2),
      speedXDelta: round(speedXDelta, 2),
      rtfDelta: round(candidate.realtimeFactor - baseline.realtimeFactor, 3),
      projectedThreeHourDeltaMin: round(
        candidate.projectedThreeHourMin - baseline.projectedThreeHourMin,
        1,
      ),
    },
  };
}

export function renderMarkdown(comparison) {
  const rows = [
    ["Baseline", comparison.baseline],
    ["Candidate", comparison.candidate],
  ];
  const output = [
    `# Diarization Benchmark Comparison: ${comparison.status.toUpperCase()}`,
    "",
    "| Run | Mode | Wall | RTF | Speed | 3h projection | Speakers | Segments |",
    "| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |",
    ...rows.map(
      ([label, report]) =>
        `| ${label} | ${report.mode} | ${report.wallClockSec.toFixed(2)}s | ${report.realtimeFactor.toFixed(3)} | ${report.speedX.toFixed(2)}x | ${report.projectedThreeHourMin.toFixed(1)} min | ${report.speakerCount} | ${report.diarizedSegmentCount} |`,
    ),
    "",
    `- Wall-clock speedup: ${comparison.metrics.speedup.toFixed(3)}x`,
    `- Wall delta: ${comparison.metrics.wallDeltaSec.toFixed(2)}s`,
    `- Speed delta: ${comparison.metrics.speedXDelta.toFixed(2)}x`,
    `- RTF delta: ${comparison.metrics.rtfDelta.toFixed(3)}`,
    `- 3h projection delta: ${comparison.metrics.projectedThreeHourDeltaMin.toFixed(1)} min`,
  ];

  if (comparison.failures.length > 0) {
    output.push("", "## Failures", ...comparison.failures.map((failure) => `- ${failure}`));
  }

  return `${output.join("\n")}\n`;
}

async function main() {
  let args;
  try {
    args = parseArgs(process.argv.slice(2));
  } catch (error) {
    console.error(error.message);
    console.error(usage());
    process.exit(1);
  }

  if (args.help) {
    console.log(usage());
    return;
  }

  if (!args.baseline || !args.candidate) {
    console.error(usage());
    process.exit(1);
  }

  const comparison = compareReports(readReport(args.baseline), readReport(args.candidate), {
    minSpeedup: args.minSpeedup,
    maxRtf: args.maxRtf,
    expectedSpeakers: args.expectedSpeakers,
  });

  if (args.format === "json") {
    console.log(JSON.stringify(comparison, null, 2));
  } else {
    process.stdout.write(renderMarkdown(comparison));
  }

  if (comparison.status !== "pass") {
    process.exit(2);
  }
}

if (import.meta.url === pathToFileURL(process.argv[1] || "").href) {
  main().catch((error) => {
    console.error(error);
    process.exit(1);
  });
}
