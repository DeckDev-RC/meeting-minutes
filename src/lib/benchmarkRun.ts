import type { BenchmarkRun } from "./evaluation";
import type {
  EvidencePurgeSummary,
  MeetingAction,
  MeetingChunkInsights,
  MeetingDecision,
  MeetingMetadata,
} from "./types";

export interface BenchmarkRunBuildInput {
  meetingId: string;
  title: string | null;
  sourcePath: string;
  processingSec: number;
  audioSec: number;
  engine: string;
  speakers: string[];
  facts: MeetingChunkInsights[];
  mediaMetadata?: MeetingMetadata;
  purgeSummary?: EvidencePurgeSummary;
}

export interface BenchmarkRunCaseMetadata {
  title: string | null;
  sourcePath: string;
  generatedAt: string;
  mediaMetadata?: MeetingMetadata;
  purgeSummary?: EvidencePurgeSummary;
}

export type BenchmarkRunWithMetadata = BenchmarkRun & {
  createdAt: string;
  cases: Array<
    BenchmarkRun["cases"][number] & {
      metadata: BenchmarkRunCaseMetadata;
    }
  >;
};

export function buildBenchmarkRun(input: BenchmarkRunBuildInput): BenchmarkRunWithMetadata {
  const createdAt = new Date().toISOString();
  const facts = [...input.facts].sort((a, b) => a.chunkIndex - b.chunkIndex);

  return {
    version: 1,
    engine: input.engine,
    createdAt,
    cases: [
      {
        caseId: input.meetingId,
        processingSec: round3(input.processingSec),
        audioSec: round3(input.audioSec),
        output: {
          speakers: input.speakers,
          summary: facts.map((fact) => fact.summary.trim()).filter(Boolean).join("\n\n"),
          topics: uniqueStrings(facts.flatMap((fact) => fact.topics)),
          decisions: facts.flatMap((fact) => fact.decisions).map(normalizeDecision),
          actions: facts.flatMap((fact) => fact.actions).map(normalizeAction),
          questions: uniqueStrings(facts.flatMap((fact) => fact.questions)),
          risks: uniqueStrings(facts.flatMap((fact) => fact.risks)),
        },
        metadata: {
          title: input.title,
          sourcePath: input.sourcePath,
          generatedAt: createdAt,
          mediaMetadata: input.mediaMetadata,
          purgeSummary: input.purgeSummary ? normalizePurgeSummary(input.purgeSummary) : undefined,
        },
      },
    ],
  };
}

export function buildBenchmarkRunFileName(meetingId: string): string {
  return `${meetingId}_benchmark-run.json`;
}

export function buildBenchmarkRunArtifactPath(outputDir: string, meetingId: string): string {
  const separator = outputDir.includes("\\") ? "\\" : "/";
  return `${outputDir.replace(/[\\/]+$/, "")}${separator}${buildBenchmarkRunFileName(meetingId)}`;
}

export function buildBenchmarkRunPath(sourcePath: string, meetingId: string): string {
  const lastSeparator = Math.max(sourcePath.lastIndexOf("\\"), sourcePath.lastIndexOf("/"));
  const dir = lastSeparator >= 0 ? sourcePath.slice(0, lastSeparator) : ".";
  const separator = sourcePath.includes("\\") ? "\\" : "/";
  return `${dir.replace(/[\\/]+$/, "")}${separator}${buildBenchmarkRunFileName(meetingId)}`;
}

function normalizePurgeSummary(summary: EvidencePurgeSummary): EvidencePurgeSummary {
  const removedTopics = nonNegativeInteger(summary.removedTopics);
  const removedDecisions = nonNegativeInteger(summary.removedDecisions);
  const removedActions = nonNegativeInteger(summary.removedActions);
  const removedTotal = nonNegativeInteger(summary.removedTotal);
  return {
    removedTopics,
    removedDecisions,
    removedActions,
    removedTotal: removedTotal || removedTopics + removedDecisions + removedActions,
  };
}

function normalizeDecision(decision: MeetingDecision): MeetingDecision {
  return {
    title: decision.title,
    owner: decision.owner,
    timestampSec: round3(decision.timestampSec),
    evidence: decision.evidence,
  };
}

function normalizeAction(action: MeetingAction): MeetingAction {
  return {
    task: action.task,
    owner: action.owner,
    deadline: action.deadline,
    timestampSec: round3(action.timestampSec),
    evidence: action.evidence,
  };
}

function uniqueStrings(values: string[]): string[] {
  const seen = new Set<string>();
  const result: string[] = [];

  for (const value of values) {
    const trimmed = value.trim();
    const key = trimmed.toLocaleLowerCase("pt-BR");
    if (!trimmed || seen.has(key)) {
      continue;
    }

    seen.add(key);
    result.push(trimmed);
  }

  return result;
}

function round3(value: number): number {
  return Math.round((value + Number.EPSILON) * 1000) / 1000;
}

function nonNegativeInteger(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.max(0, Math.floor(value));
}
