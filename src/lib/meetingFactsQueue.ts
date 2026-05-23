import type {
  MeetingChunkInsights,
  DiarizedSegment,
  ProcessingChunkRecord,
  TranscriptionSegment,
} from './types';
import { sanitizeMeetingChunkInsights } from './minutesEvidence';

export interface MeetingFactsDoneEvent {
  chunkIndex: number;
  completedChunks: number;
  totalChunks: number;
  cachedChunks: number;
  extractedChunks: number;
}

export type FactSourceSegment = TranscriptionSegment | DiarizedSegment;

export type SegmentParser = (chunk: ProcessingChunkRecord) => FactSourceSegment[];

export type ChunkFactExtractor = (
  chunk: ProcessingChunkRecord,
  segments: FactSourceSegment[],
  apiKey: string
) => Promise<MeetingChunkInsights>;

export interface FactBatchItem {
  chunk: ProcessingChunkRecord;
  segments: FactSourceSegment[];
  estimatedChars: number;
}

export interface FactBatch {
  items: FactBatchItem[];
  estimatedChars: number;
  startSec: number;
  endSec: number;
}

export type BatchFactExtractor = (
  batch: FactBatchItem[],
  apiKey: string
) => Promise<MeetingChunkInsights[]>;

export type ChunkFactUpdater = (
  chunk: ProcessingChunkRecord,
  status: ProcessingChunkRecord['factsStatus'],
  factsJson?: string,
  errorMsg?: string
) => Promise<void>;

export interface MeetingFactsQueueOptions {
  chunks: ProcessingChunkRecord[];
  apiKey: string;
  concurrency: number;
  parseSegments: SegmentParser;
  extractChunkFacts?: ChunkFactExtractor;
  extractFactBatch?: BatchFactExtractor;
  maxBatchChars?: number;
  updateChunkFacts?: ChunkFactUpdater;
  onChunkDone?: (event: MeetingFactsDoneEvent) => void;
}

const persistFactStatus = (
  updateChunkFacts: ChunkFactUpdater | undefined,
  chunk: ProcessingChunkRecord,
  status: ProcessingChunkRecord['factsStatus'],
  factsJson?: string,
  errorMsg?: string,
) => updateChunkFacts?.(chunk, status, factsJson, errorMsg) ?? Promise.resolve();

export interface AdaptiveFactBatchOptions {
  chunks: ProcessingChunkRecord[];
  parseSegments: SegmentParser;
  maxBatchChars?: number;
}

const DEFAULT_MAX_BATCH_CHARS = 9000;
const MAX_SEGMENT_TEXT_CHARS = 700;

function formatError(error: unknown) {
  if (error instanceof Error) return error.message;
  if (typeof error === 'string') return error;
  return String(error);
}

function parseCachedFacts(chunk: ProcessingChunkRecord): MeetingChunkInsights | null {
  if (chunk.factsStatus !== 'done' || !chunk.factsJson) {
    return null;
  }

  try {
    const parsed = sanitizeMeetingChunkInsights(JSON.parse(chunk.factsJson) as unknown);
    if (
      typeof parsed.chunkIndex === 'number' &&
      typeof parsed.startSec === 'number' &&
      typeof parsed.endSec === 'number' &&
      typeof parsed.summary === 'string' &&
      Array.isArray(parsed.topics) &&
      Array.isArray(parsed.decisions) &&
      Array.isArray(parsed.actions) &&
      Array.isArray(parsed.questions) &&
      Array.isArray(parsed.risks)
    ) {
      return parsed;
    }
  } catch {
    return null;
  }

  return null;
}

function normalizeTextKey(text: string) {
  return text.trim().toLocaleLowerCase('pt-BR').replace(/\s+/g, ' ');
}

function truncateText(text: string, maxChars: number) {
  const trimmed = text.trim();
  return trimmed.length > maxChars ? `${trimmed.slice(0, maxChars).trim()}...` : trimmed;
}

function compactFactSegments(segments: FactSourceSegment[]): FactSourceSegment[] {
  const seen = new Set<string>();
  const compacted: FactSourceSegment[] = [];

  for (const segment of segments) {
    const sourceText = segment.text || '';
    const text = truncateText(sourceText, MAX_SEGMENT_TEXT_CHARS);
    const key = normalizeTextKey(text);
    if (!key || seen.has(key)) {
      continue;
    }

    seen.add(key);
    compacted.push(text === sourceText ? segment : { ...segment, text });
  }

  return compacted;
}

function estimateSegmentsChars(segments: FactSourceSegment[]) {
  return segments.reduce((sum, segment) => sum + segment.text.length + 32, 0);
}

function factsInChunkOrder(
  chunks: ProcessingChunkRecord[],
  results: Map<number, MeetingChunkInsights>
) {
  return chunks.flatMap((chunk) => results.get(chunk.index) ?? []);
}

function fallbackInsightsForChunk(
  chunk: ProcessingChunkRecord,
  segments: FactSourceSegment[]
): MeetingChunkInsights {
  const text = segments.map((segment) => segment.text.trim()).filter(Boolean).join(' ');
  return {
    chunkIndex: chunk.index,
    startSec: chunk.startSec,
    endSec: chunk.endSec,
    summary: text ? truncateText(text, 420) : 'Trecho sem fatos estruturados extraidos.',
    topics: text ? ['Trecho da reuniao'] : [],
    decisions: [],
    actions: [],
    questions: [],
    risks: [],
  };
}

export function buildAdaptiveFactBatches({
  chunks,
  parseSegments,
  maxBatchChars = DEFAULT_MAX_BATCH_CHARS,
}: AdaptiveFactBatchOptions): FactBatch[] {
  const safeMaxChars =
    Number.isFinite(maxBatchChars) && maxBatchChars > 0 ? Math.floor(maxBatchChars) : DEFAULT_MAX_BATCH_CHARS;
  const batches: FactBatch[] = [];
  let current: FactBatchItem[] = [];
  let currentChars = 0;
  let currentStartSec = Number.POSITIVE_INFINITY;
  let currentEndSec = 0;

  const flush = () => {
    if (current.length === 0) return;
    batches.push({
      items: current,
      estimatedChars: currentChars,
      startSec: currentStartSec,
      endSec: currentEndSec,
    });
    current = [];
    currentChars = 0;
    currentStartSec = Number.POSITIVE_INFINITY;
    currentEndSec = 0;
  };

  for (const chunk of chunks) {
    const segments = compactFactSegments(parseSegments(chunk));
    const estimatedChars = estimateSegmentsChars(segments);
    const item: FactBatchItem = { chunk, segments, estimatedChars };

    if (current.length > 0 && currentChars + estimatedChars > safeMaxChars) {
      flush();
    }

    current.push(item);
    currentChars += estimatedChars;
    currentStartSec = Math.min(currentStartSec, chunk.startSec);
    currentEndSec = Math.max(currentEndSec, chunk.endSec);
  }

  flush();
  return batches;
}

export async function extractMeetingFactsConcurrently({
  chunks,
  apiKey,
  concurrency,
  parseSegments,
  extractChunkFacts,
  extractFactBatch,
  maxBatchChars,
  updateChunkFacts,
  onChunkDone,
}: MeetingFactsQueueOptions): Promise<MeetingChunkInsights[]> {
  if (chunks.length === 0) {
    return [];
  }

  const results = new Map<number, MeetingChunkInsights>();
  const pendingChunks: ProcessingChunkRecord[] = [];
  const totalChunks = chunks.length;
  let completedChunks = 0;
  let cachedChunks = 0;
  let extractedChunks = 0;

  const notifyDone = (chunkIndex: number) => {
    onChunkDone?.({
      chunkIndex,
      completedChunks,
      totalChunks,
      cachedChunks,
      extractedChunks,
    });
  };

  for (const chunk of chunks) {
    const cached = parseCachedFacts(chunk);
    if (cached) {
      results.set(chunk.index, cached);
      cachedChunks += 1;
      completedChunks += 1;
      notifyDone(chunk.index);
    } else {
      pendingChunks.push(chunk);
    }
  }

  if (pendingChunks.length === 0) {
    return factsInChunkOrder(chunks, results);
  }

  const requestedConcurrency =
    Number.isFinite(concurrency) && concurrency >= 1 ? Math.floor(concurrency) : 1;
  const safeConcurrency = Math.min(requestedConcurrency, pendingChunks.length);
  let nextIndex = 0;
  let failed = false;

  if (extractFactBatch) {
    const batchExtractor = extractFactBatch!;
    const batches = buildAdaptiveFactBatches({ chunks: pendingChunks, parseSegments, maxBatchChars });
    const safeBatchConcurrency = Math.min(requestedConcurrency, batches.length);

    async function batchWorker() {
      while (!failed && nextIndex < batches.length) {
        const batch = batches[nextIndex];
        nextIndex += 1;

        try {
          const extracted = await batchExtractor(batch.items, apiKey);
          const byChunkIndex = new Map(extracted.map((insight) => [insight.chunkIndex, insight]));

          if (failed) {
            return;
          }

          await Promise.all(batch.items.map(async (item) => {
            const insights = sanitizeMeetingChunkInsights(
              byChunkIndex.get(item.chunk.index) ?? fallbackInsightsForChunk(item.chunk, item.segments)
            );
            await persistFactStatus(updateChunkFacts, item.chunk, 'done', JSON.stringify(insights));
            results.set(item.chunk.index, insights);
            extractedChunks += 1;
            completedChunks += 1;
            notifyDone(item.chunk.index);
          }));
        } catch (error) {
          failed = true;
          await Promise.all(
            batch.items.map((item) =>
              persistFactStatus(updateChunkFacts, item.chunk, 'error', undefined, formatError(error)).catch(() => {})
            )
          );
          throw error;
        }
      }
    }

    await Promise.all(Array.from({ length: safeBatchConcurrency }, () => batchWorker()));

    return factsInChunkOrder(chunks, results);
  }

  if (!extractChunkFacts) {
    throw new Error('extractChunkFacts or extractFactBatch is required');
  }
  const chunkExtractor = extractChunkFacts!;

  async function worker() {
    while (!failed && nextIndex < pendingChunks.length) {
      const chunk = pendingChunks[nextIndex];
      nextIndex += 1;
      let runningPersist = Promise.resolve();

      try {
        runningPersist = persistFactStatus(updateChunkFacts, chunk, 'running').catch(() => {});
        const segments = parseSegments(chunk);
        const insights = sanitizeMeetingChunkInsights(await chunkExtractor(chunk, segments, apiKey));

        if (failed) {
          return;
        }

        await runningPersist;
        await persistFactStatus(updateChunkFacts, chunk, 'done', JSON.stringify(insights));
        results.set(chunk.index, insights);
        extractedChunks += 1;
        completedChunks += 1;
        notifyDone(chunk.index);
      } catch (error) {
        failed = true;
        await runningPersist;
        await persistFactStatus(updateChunkFacts, chunk, 'error', undefined, formatError(error)).catch(() => {});
        throw error;
      }
    }
  }

  await Promise.all(Array.from({ length: safeConcurrency }, () => worker()));

  return factsInChunkOrder(chunks, results);
}
