import type { BenchmarkManifest, BenchmarkThresholds } from "./evaluation";
import type { MeetingChunkInsights } from "./types";

export type TextBenchmarkDataset = "meetingbank" | "publichearingbr";

export interface TextBenchmarkOptions {
  maxCases?: number;
  transcriptCharLimit?: number;
}

export interface TextChunkOptions {
  targetChars?: number;
}

export interface HuggingFaceRowsResponse {
  rows?: Array<{
    row_idx: number;
    row: Record<string, unknown>;
  }>;
}

export interface TextBenchmarkCase {
  id: string;
  title: string;
  dataset: string;
  datasetKey: TextBenchmarkDataset;
  language: string;
  transcript: string;
  sourceRef: string;
  referenceSummary: string;
  estimatedDurationSec: number;
}

export interface TextBenchmarkChunk {
  chunkIndex: number;
  startSec: number;
  endSec: number;
  text: string;
}

const DEFAULT_WPM = 150;
const DEFAULT_TARGET_CHARS = 6000;
const STOPWORDS = new Set([
  "about",
  "after",
  "also",
  "and",
  "are",
  "because",
  "been",
  "being",
  "com",
  "como",
  "das",
  "dos",
  "for",
  "from",
  "have",
  "into",
  "para",
  "pela",
  "pelo",
  "que",
  "sobre",
  "the",
  "their",
  "this",
  "uma",
  "with",
]);

export function extractTextBenchmarkCases(
  dataset: TextBenchmarkDataset,
  response: HuggingFaceRowsResponse,
  options: TextBenchmarkOptions = {},
): TextBenchmarkCase[] {
  const rows = response.rows ?? [];
  const cases: TextBenchmarkCase[] = [];
  const maxCases = options.maxCases ?? rows.length;

  for (const item of rows) {
    const parsed = dataset === "meetingbank" ? parseMeetingBankRow(item.row) : parsePublicHearingRow(item.row);
    if (!parsed) {
      continue;
    }

    const transcript = limitText(parsed.transcript, options.transcriptCharLimit);
    if (!transcript) {
      continue;
    }

    cases.push({
      ...parsed,
      transcript,
      estimatedDurationSec: estimateDurationSec(transcript),
    });

    if (cases.length >= maxCases) {
      break;
    }
  }

  return cases;
}

export function chunkTextBenchmarkCase(
  benchmarkCase: TextBenchmarkCase,
  options: TextChunkOptions = {},
): TextBenchmarkChunk[] {
  const targetChars = Math.max(200, options.targetChars ?? DEFAULT_TARGET_CHARS);
  const pieces = splitTextIntoPieces(benchmarkCase.transcript, targetChars);
  const totalChars = pieces.reduce((sum, piece) => sum + piece.length, 0);
  let elapsedSec = 0;

  return pieces.map((piece, index) => {
    const isLast = index === pieces.length - 1;
    const duration =
      isLast || totalChars === 0
        ? benchmarkCase.estimatedDurationSec - elapsedSec
        : (piece.length / totalChars) * benchmarkCase.estimatedDurationSec;
    const startSec = round3(elapsedSec);
    elapsedSec += duration;
    const endSec = isLast ? benchmarkCase.estimatedDurationSec : round3(elapsedSec);

    return {
      chunkIndex: index,
      startSec,
      endSec,
      text: piece,
    };
  });
}

export function buildGeminiChunkFactsPrompt(
  chunk: TextBenchmarkChunk,
  benchmarkCase: TextBenchmarkCase,
): string {
  return [
    "You extract meeting facts for a benchmark. Return only valid JSON. Do not use markdown.",
    "",
    "Schema:",
    '{"chunkIndex":number,"startSec":number,"endSec":number,"summary":"string","topics":["string"],"topicEvidence":[{"title":"string","timestampSec":number,"evidence":"string"}],"decisions":[{"title":"string","owner":"string","timestampSec":number,"evidence":"string"}],"actions":[{"task":"string","owner":"string","deadline":"string","timestampSec":number,"evidence":"string"}],"questions":["string"],"risks":["string"]}',
    "",
    "Rules:",
    "- Keep summary under 240 characters.",
    "- Use empty arrays when no item is explicit.",
    "- Add one topicEvidence item for each topic, with title exactly matching the topic.",
    "- Evidence must be short and copied or closely paraphrased from the transcript.",
    "- Use the transcript language.",
    "",
    `Dataset: ${benchmarkCase.dataset}`,
    `Language: ${benchmarkCase.language}`,
    `chunkIndex: ${chunk.chunkIndex}`,
    `startSec: ${chunk.startSec}`,
    `endSec: ${chunk.endSec}`,
    "",
    "Transcript:",
    chunk.text,
  ].join("\n");
}

export function buildGeminiFinalMinutesPrompt(
  benchmarkCase: TextBenchmarkCase,
  facts: MeetingChunkInsights[],
): string {
  return [
    "Create a concise professional meeting minutes document in HTML.",
    "Use only the structured facts below. Do not invent decisions, actions, owners, deadlines, or risks.",
    "Return only HTML, without markdown fences.",
    "",
    `Title: ${benchmarkCase.title}`,
    `Language: ${benchmarkCase.language}`,
    "",
    "Facts JSON:",
    JSON.stringify(facts),
  ].join("\n");
}

export function parseGeminiChunkFactsText(
  text: string,
  chunk: TextBenchmarkChunk,
): MeetingChunkInsights {
  const jsonText = extractJsonObject(text);
  if (jsonText) {
    try {
      const parsed = JSON.parse(jsonText) as Partial<MeetingChunkInsights>;
      return normalizeInsights(parsed, chunk);
    } catch {
      // Fall through to deterministic fallback.
    }
  }

  return fallbackInsights(chunk);
}

export function buildDraftManifestFromTextCases(
  cases: TextBenchmarkCase[],
  thresholds: BenchmarkThresholds = {},
): BenchmarkManifest {
  return {
    version: 1,
    thresholds,
    cases: cases.map((item) => ({
      id: item.id,
      title: item.title,
      dataset: item.dataset,
      language: item.language,
      durationSec: item.estimatedDurationSec,
      referenceItems: item.referenceSummary
        ? [
            {
              id: `${item.id}-summary`,
              kind: "summary",
              text: item.referenceSummary,
              requiredTerms: extractRequiredTerms(item.referenceSummary, 8),
            },
          ]
        : [],
    })),
  };
}

export function estimateDurationSec(transcript: string, wordsPerMinute = DEFAULT_WPM): number {
  const words = transcript.trim().split(/\s+/).filter(Boolean).length;
  return Math.max(1, round3((words / wordsPerMinute) * 60));
}

function parseMeetingBankRow(row: Record<string, unknown>): Omit<TextBenchmarkCase, "estimatedDurationSec"> | null {
  const transcript = stringValue(row.transcript);
  if (!transcript) {
    return null;
  }

  const uid = stringValue(row.uid) || `row-${stringValue(row.id) || "unknown"}`;
  return {
    id: `meetingbank-${safeId(uid)}`,
    title: uid,
    dataset: "MeetingBank",
    datasetKey: "meetingbank",
    language: "en",
    transcript,
    sourceRef: `hf://datasets/huuuyeah/meetingbank/${uid}`,
    referenceSummary: stringValue(row.summary),
  };
}

function parsePublicHearingRow(row: Record<string, unknown>): Omit<TextBenchmarkCase, "estimatedDurationSec"> | null {
  const transcript = stringValue(row.transcricao);
  if (!transcript) {
    return null;
  }

  const id = stringValue(row.id) || "unknown";
  const materia = stringValue(row.materia);
  const metadata = row.metadados && typeof row.metadados === "object" ? (row.metadados as Record<string, unknown>) : {};
  const subject = stringValue(metadata.assunto);

  return {
    id: `publichearingbr-${safeId(id)}`,
    title: subject || firstLine(materia) || `PublicHearingBR ${id}`,
    dataset: "PublicHearingBR",
    datasetKey: "publichearingbr",
    language: "pt-BR",
    transcript,
    sourceRef: `hf://datasets/unicamp-dl/PublicHearingBR/${id}`,
    referenceSummary: materia,
  };
}

function splitTextIntoPieces(text: string, targetChars: number): string[] {
  const sentences = text
    .split(/(?<=[.!?])\s+/)
    .map((item) => item.trim())
    .filter(Boolean);
  const pieces: string[] = [];
  let current = "";

  for (const sentence of sentences.length > 0 ? sentences : [text]) {
    if (!current) {
      current = sentence;
      continue;
    }

    if (current.length + 1 + sentence.length <= targetChars) {
      current = `${current} ${sentence}`;
    } else {
      pieces.push(current);
      current = sentence;
    }
  }

  if (current) {
    pieces.push(current);
  }

  return pieces.flatMap((piece) => splitOversizedPiece(piece, targetChars));
}

function splitOversizedPiece(text: string, targetChars: number): string[] {
  if (text.length <= targetChars) {
    return [text];
  }

  const words = text.split(/\s+/).filter(Boolean);
  const chunks: string[] = [];
  let current = "";

  for (const word of words) {
    if (!current) {
      current = word;
      continue;
    }

    if (current.length + 1 + word.length <= targetChars) {
      current = `${current} ${word}`;
    } else {
      chunks.push(current);
      current = word;
    }
  }

  if (current) {
    chunks.push(current);
  }

  return chunks;
}

function normalizeInsights(
  parsed: Partial<MeetingChunkInsights>,
  chunk: TextBenchmarkChunk,
): MeetingChunkInsights {
  return {
    chunkIndex: chunk.chunkIndex,
    startSec: chunk.startSec,
    endSec: chunk.endSec,
    summary: stringValue(parsed.summary) || fallbackSummary(chunk.text),
    topics: stringArray(parsed.topics).slice(0, 8),
    topicEvidence: Array.isArray(parsed.topicEvidence)
      ? parsed.topicEvidence.map(normalizeTopicEvidence).slice(0, 8)
      : undefined,
    decisions: Array.isArray(parsed.decisions) ? parsed.decisions.map(normalizeDecision).slice(0, 8) : [],
    actions: Array.isArray(parsed.actions) ? parsed.actions.map(normalizeAction).slice(0, 12) : [],
    questions: stringArray(parsed.questions).slice(0, 8),
    risks: stringArray(parsed.risks).slice(0, 8),
  };
}

function normalizeTopicEvidence(value: unknown) {
  const item = value && typeof value === "object" ? (value as Record<string, unknown>) : {};
  return {
    title: stringValue(item.title),
    timestampSec: numberValue(item.timestampSec),
    evidence: stringValue(item.evidence).slice(0, 160),
  };
}

function normalizeDecision(value: unknown) {
  const item = value && typeof value === "object" ? (value as Record<string, unknown>) : {};
  return {
    title: stringValue(item.title),
    owner: stringValue(item.owner),
    timestampSec: numberValue(item.timestampSec),
    evidence: stringValue(item.evidence).slice(0, 160),
  };
}

function normalizeAction(value: unknown) {
  const item = value && typeof value === "object" ? (value as Record<string, unknown>) : {};
  return {
    task: stringValue(item.task),
    owner: stringValue(item.owner),
    deadline: stringValue(item.deadline),
    timestampSec: numberValue(item.timestampSec),
    evidence: stringValue(item.evidence).slice(0, 160),
  };
}

function fallbackInsights(chunk: TextBenchmarkChunk): MeetingChunkInsights {
  return {
    chunkIndex: chunk.chunkIndex,
    startSec: chunk.startSec,
    endSec: chunk.endSec,
    summary: fallbackSummary(chunk.text),
    topics: [],
    decisions: [],
    actions: [],
    questions: [],
    risks: [],
  };
}

function fallbackSummary(text: string): string {
  return text.replace(/\s+/g, " ").trim().slice(0, 240);
}

function extractJsonObject(text: string): string | null {
  const fenced = text.match(/```(?:json)?\s*([\s\S]*?)```/i);
  const candidate = fenced ? fenced[1] : text;
  const start = candidate.indexOf("{");
  const end = candidate.lastIndexOf("}");

  if (start < 0 || end <= start) {
    return null;
  }

  return candidate.slice(start, end + 1);
}

function extractRequiredTerms(text: string, maxTerms: number): string[] {
  const counts = new Map<string, number>();
  for (const token of text
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLowerCase()
    .match(/[a-z0-9]{4,}/g) ?? []) {
    if (STOPWORDS.has(token)) {
      continue;
    }

    counts.set(token, (counts.get(token) ?? 0) + 1);
  }

  return [...counts.entries()]
    .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
    .slice(0, maxTerms)
    .map(([term]) => term);
}

function limitText(text: string, limit?: number): string {
  const cleaned = text.replace(/\s+/g, " ").trim();
  if (!limit || cleaned.length <= limit) {
    return cleaned;
  }

  return cleaned.slice(0, limit).trim();
}

function stringArray(value: unknown): string[] {
  return Array.isArray(value) ? value.map(stringValue).filter(Boolean) : [];
}

function stringValue(value: unknown): string {
  if (typeof value === "string") {
    return value.trim();
  }

  if (typeof value === "number") {
    return String(value);
  }

  return "";
}

function numberValue(value: unknown): number {
  return typeof value === "number" && Number.isFinite(value) ? round3(value) : 0;
}

function firstLine(value: string): string {
  return value.split(/\r?\n/).map((item) => item.trim()).find(Boolean) ?? "";
}

function safeId(value: string): string {
  return value.replace(/[^A-Za-z0-9_-]+/g, "_").replace(/^_+|_+$/g, "") || "unknown";
}

function round3(value: number): number {
  return Math.round((value + Number.EPSILON) * 1000) / 1000;
}
