import type { MeetingAction, MeetingDecision } from "./types";

export type BenchmarkItemKind = "decision" | "action" | "question" | "risk" | "topic" | "summary";
export type BenchmarkStatus = "pass" | "fail";

export interface BenchmarkThresholds {
  minThroughputX?: number;
  maxRealTimeFactor?: number;
  minFactRecall?: number;
  minFactPrecision?: number;
}

export interface BenchmarkReferenceItem {
  id: string;
  kind: BenchmarkItemKind;
  text: string;
  requiredTerms: string[];
  weight?: number;
}

export interface BenchmarkCase {
  id: string;
  title: string;
  dataset: string;
  language: string;
  durationSec: number;
  expectedSpeakers?: number;
  referenceItems: BenchmarkReferenceItem[];
}

export interface BenchmarkManifest {
  version: number;
  thresholds?: BenchmarkThresholds;
  cases: BenchmarkCase[];
}

export interface BenchmarkOutput {
  speakers?: string[];
  summary?: string;
  topics?: string[];
  decisions?: MeetingDecision[];
  actions?: MeetingAction[];
  questions?: string[];
  risks?: string[];
}

export interface BenchmarkRunCase {
  caseId: string;
  processingSec: number;
  audioSec?: number;
  output: BenchmarkOutput;
}

export interface BenchmarkRun {
  version: number;
  engine?: string;
  cases: BenchmarkRunCase[];
}

export interface ScoreBreakdown {
  referenceCount: number;
  predictionCount: number;
  matchedCount: number;
  precision: number;
  recall: number;
  f1: number;
}

export interface FactScore {
  overall: ScoreBreakdown;
  byKind: Record<BenchmarkItemKind, ScoreBreakdown>;
}

export interface SpeedScore {
  audioSec: number;
  processingSec: number;
  throughputX: number;
  realTimeFactor: number;
}

export interface BenchmarkCaseReport {
  caseId: string;
  title: string;
  dataset: string;
  speed: SpeedScore;
  factScore: FactScore;
  speakerDelta: number | null;
  missedReferenceIds: string[];
  unmatchedPredictionCount: number;
}

export interface BenchmarkReport {
  status: BenchmarkStatus;
  aggregate: {
    speed: SpeedScore;
    factScore: ScoreBreakdown;
  };
  cases: BenchmarkCaseReport[];
  gateFailures: string[];
}

const ITEM_KINDS: BenchmarkItemKind[] = ["decision", "action", "question", "risk", "topic", "summary"];

export function normalizeEvaluationText(value: string): string {
  return value
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, " ")
    .trim()
    .replace(/\s+/g, " ");
}

export function evaluateBenchmarkRun(manifest: BenchmarkManifest, run: BenchmarkRun): BenchmarkReport {
  const casesById = new Map(manifest.cases.map((item) => [item.id, item]));
  const reports = run.cases.map((runCase) => {
    const benchmarkCase = casesById.get(runCase.caseId);
    if (!benchmarkCase) {
      throw new Error(`Benchmark case not found in manifest: ${runCase.caseId}`);
    }

    return evaluateCase(benchmarkCase, runCase);
  });

  const aggregateSpeed = buildSpeedScore(
    sum(reports.map((item) => item.speed.audioSec)),
    sum(reports.map((item) => item.speed.processingSec)),
  );
  const aggregateFactScore = buildBreakdown(
    sum(reports.map((item) => item.factScore.overall.referenceCount)),
    sum(reports.map((item) => item.factScore.overall.predictionCount)),
    sum(reports.map((item) => item.factScore.overall.matchedCount)),
  );
  const gateFailures = evaluateThresholds(manifest.thresholds ?? {}, aggregateSpeed, aggregateFactScore);

  return {
    status: gateFailures.length === 0 ? "pass" : "fail",
    aggregate: {
      speed: aggregateSpeed,
      factScore: aggregateFactScore,
    },
    cases: reports,
    gateFailures,
  };
}

export function renderBenchmarkReportMarkdown(report: BenchmarkReport): string {
  const lines = [
    `# Benchmark report: ${report.status.toUpperCase()}`,
    "",
    `Aggregate speed: ${formatNumber(report.aggregate.speed.throughputX, 2)}x (${formatNumber(report.aggregate.speed.realTimeFactor, 3)} RTF)`,
    formatFactsSummary(report.aggregate.factScore),
    "",
    "| Case | Dataset | Speed | Precision | Recall | F1 | Missed | Extra |",
    "| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |",
  ];

  for (const item of report.cases) {
    lines.push(
      `| ${item.caseId} | ${item.dataset} | ${formatNumber(item.speed.throughputX, 2)}x | ${formatScorePercent(item.factScore.overall, "precision")} | ${formatScorePercent(item.factScore.overall, "recall")} | ${formatScorePercent(item.factScore.overall, "f1")} | ${item.missedReferenceIds.length} | ${item.unmatchedPredictionCount} |`,
    );
  }

  if (report.gateFailures.length > 0) {
    lines.push("", "Gate failures:");
    for (const failure of report.gateFailures) {
      lines.push(`- ${failure}`);
    }
  }

  return `${lines.join("\n")}\n`;
}

function evaluateCase(benchmarkCase: BenchmarkCase, runCase: BenchmarkRunCase): BenchmarkCaseReport {
  const speed = buildSpeedScore(runCase.audioSec ?? benchmarkCase.durationSec, runCase.processingSec);
  const { factScore, missedReferenceIds, unmatchedPredictionCount } = scoreFacts(
    benchmarkCase.referenceItems,
    runCase.output,
  );
  const actualSpeakers = runCase.output.speakers?.length;
  const speakerDelta =
    typeof benchmarkCase.expectedSpeakers === "number" && typeof actualSpeakers === "number"
      ? actualSpeakers - benchmarkCase.expectedSpeakers
      : null;

  return {
    caseId: benchmarkCase.id,
    title: benchmarkCase.title,
    dataset: benchmarkCase.dataset,
    speed,
    factScore,
    speakerDelta,
    missedReferenceIds,
    unmatchedPredictionCount,
  };
}

function scoreFacts(
  references: BenchmarkReferenceItem[],
  output: BenchmarkOutput,
): {
  factScore: FactScore;
  missedReferenceIds: string[];
  unmatchedPredictionCount: number;
} {
  const byKind = Object.fromEntries(
    ITEM_KINDS.map((kind) => [kind, scoreKind(kind, references, output)]),
  ) as Record<BenchmarkItemKind, ScoreBreakdown & { missedReferenceIds: string[]; unmatchedPredictionCount: number }>;

  const overall = buildBreakdown(
    sum(ITEM_KINDS.map((kind) => byKind[kind].referenceCount)),
    sum(ITEM_KINDS.map((kind) => byKind[kind].predictionCount)),
    sum(ITEM_KINDS.map((kind) => byKind[kind].matchedCount)),
  );

  return {
    factScore: {
      overall,
      byKind: Object.fromEntries(
        ITEM_KINDS.map((kind) => [
          kind,
          {
            referenceCount: byKind[kind].referenceCount,
            predictionCount: byKind[kind].predictionCount,
            matchedCount: byKind[kind].matchedCount,
            precision: byKind[kind].precision,
            recall: byKind[kind].recall,
            f1: byKind[kind].f1,
          },
        ]),
      ) as Record<BenchmarkItemKind, ScoreBreakdown>,
    },
    missedReferenceIds: ITEM_KINDS.flatMap((kind) => byKind[kind].missedReferenceIds),
    unmatchedPredictionCount: sum(ITEM_KINDS.map((kind) => byKind[kind].unmatchedPredictionCount)),
  };
}

function scoreKind(
  kind: BenchmarkItemKind,
  references: BenchmarkReferenceItem[],
  output: BenchmarkOutput,
): ScoreBreakdown & { missedReferenceIds: string[]; unmatchedPredictionCount: number } {
  const relevantReferences = references.filter((item) => item.kind === kind);
  if (relevantReferences.length === 0) {
    return {
      ...buildBreakdown(0, 0, 0),
      missedReferenceIds: [],
      unmatchedPredictionCount: 0,
    };
  }

  const predictions = collectPredictionTexts(kind, output).map((text) => normalizeEvaluationText(text));
  const usedPredictionIndexes = new Set<number>();
  const missedReferenceIds: string[] = [];

  for (const reference of relevantReferences) {
    const terms = (reference.requiredTerms.length > 0 ? reference.requiredTerms : [reference.text])
      .map((term) => normalizeEvaluationText(term))
      .filter(Boolean);
    const matchIndex = predictions.findIndex((prediction, index) => {
      if (usedPredictionIndexes.has(index)) {
        return false;
      }

      return terms.every((term) => prediction.includes(term));
    });

    if (matchIndex >= 0) {
      usedPredictionIndexes.add(matchIndex);
    } else {
      missedReferenceIds.push(reference.id);
    }
  }

  return {
    ...buildBreakdown(relevantReferences.length, predictions.length, usedPredictionIndexes.size),
    missedReferenceIds,
    unmatchedPredictionCount: predictions.length - usedPredictionIndexes.size,
  };
}

function collectPredictionTexts(kind: BenchmarkItemKind, output: BenchmarkOutput): string[] {
  if (kind === "decision") {
    return (output.decisions ?? []).map((item) => joinText(item.title, item.owner, item.evidence));
  }

  if (kind === "action") {
    return (output.actions ?? []).map((item) => joinText(item.task, item.owner, item.deadline, item.evidence));
  }

  if (kind === "question") {
    return output.questions ?? [];
  }

  if (kind === "risk") {
    return output.risks ?? [];
  }

  if (kind === "topic") {
    return output.topics ?? [];
  }

  return output.summary ? [output.summary] : [];
}

function buildSpeedScore(audioSec: number, processingSec: number): SpeedScore {
  const safeAudioSec = Math.max(0, audioSec);
  const safeProcessingSec = Math.max(0, processingSec);
  return {
    audioSec: round3(safeAudioSec),
    processingSec: round3(safeProcessingSec),
    throughputX: safeProcessingSec > 0 ? round3(safeAudioSec / safeProcessingSec) : 0,
    realTimeFactor: safeAudioSec > 0 ? round3(safeProcessingSec / safeAudioSec) : 0,
  };
}

function buildBreakdown(referenceCount: number, predictionCount: number, matchedCount: number): ScoreBreakdown {
  const precision = predictionCount === 0 ? (referenceCount === 0 ? 1 : 0) : matchedCount / predictionCount;
  const recall = referenceCount === 0 ? 1 : matchedCount / referenceCount;
  const f1 = precision + recall === 0 ? 0 : (2 * precision * recall) / (precision + recall);

  return {
    referenceCount,
    predictionCount,
    matchedCount,
    precision: round3(precision),
    recall: round3(recall),
    f1: round3(f1),
  };
}

function evaluateThresholds(
  thresholds: BenchmarkThresholds,
  speed: SpeedScore,
  facts: ScoreBreakdown,
): string[] {
  const failures: string[] = [];

  if (typeof thresholds.minThroughputX === "number" && speed.throughputX < thresholds.minThroughputX) {
    failures.push(
      `aggregate throughput ${formatNumber(speed.throughputX, 2)}x < ${formatNumber(thresholds.minThroughputX, 2)}x`,
    );
  }

  if (typeof thresholds.maxRealTimeFactor === "number" && speed.realTimeFactor > thresholds.maxRealTimeFactor) {
    failures.push(
      `aggregate RTF ${formatNumber(speed.realTimeFactor, 3)} > ${formatNumber(thresholds.maxRealTimeFactor, 3)}`,
    );
  }

  if (typeof thresholds.minFactRecall === "number" && facts.recall < thresholds.minFactRecall) {
    failures.push(
      `aggregate fact recall ${formatPercent(facts.recall)} < ${formatPercent(thresholds.minFactRecall)}`,
    );
  }

  if (typeof thresholds.minFactPrecision === "number" && facts.precision < thresholds.minFactPrecision) {
    failures.push(
      `aggregate fact precision ${formatPercent(facts.precision)} < ${formatPercent(thresholds.minFactPrecision)}`,
    );
  }

  return failures;
}

function joinText(...parts: Array<string | number | null | undefined>): string {
  return parts.filter((part) => part !== null && part !== undefined && String(part).trim()).join(" ");
}

function sum(values: number[]): number {
  return values.reduce((total, value) => total + value, 0);
}

function round3(value: number): number {
  return Math.round((value + Number.EPSILON) * 1000) / 1000;
}

function formatNumber(value: number, fractionDigits: number): string {
  return value.toFixed(fractionDigits);
}

function formatFactsSummary(score: ScoreBreakdown): string {
  if (score.referenceCount === 0) {
    return "Aggregate facts: n/a (no reference items)";
  }

  return `Aggregate facts: precision ${formatPercent(score.precision)}, recall ${formatPercent(score.recall)}, F1 ${formatPercent(score.f1)}`;
}

function formatScorePercent(score: ScoreBreakdown, key: "precision" | "recall" | "f1"): string {
  return score.referenceCount === 0 ? "n/a" : formatPercent(score[key]);
}

function formatPercent(value: number): string {
  return `${(value * 100).toFixed(1)}%`;
}
