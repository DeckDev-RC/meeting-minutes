import type { ExportedChunk, TranscriptionSegment } from './types';

export interface ChunkDoneEvent {
  chunkIndex: number;
  completedChunks: number;
  totalChunks: number;
  completedAudioSec: number;
  totalAudioSec: number;
}

export type ChunkTranscriber = (
  audioPath: string,
  apiKey: string,
  offsetSec: number
) => Promise<TranscriptionSegment[]>;

export interface TranscriptionQueueOptions {
  chunks: ExportedChunk[];
  apiKey: string;
  concurrency: number;
  transcribeChunk: ChunkTranscriber;
  onChunkDone?: (event: ChunkDoneEvent) => void;
}

function mergeSegmentsByStart(
  chunks: ExportedChunk[],
  results: Map<number, TranscriptionSegment[]>
) {
  const lists = chunks.map((chunk) => results.get(chunk.index) ?? []);
  const cursors = lists.map(() => 0);
  const merged: TranscriptionSegment[] = [];
  const heap: Array<{ listIndex: number; segment: TranscriptionSegment }> = [];

  const isBefore = (
    left: { listIndex: number; segment: TranscriptionSegment },
    right: { listIndex: number; segment: TranscriptionSegment }
  ) =>
    left.segment.start < right.segment.start ||
    (left.segment.start === right.segment.start && left.listIndex < right.listIndex);

  const pushHeap = (item: { listIndex: number; segment: TranscriptionSegment }) => {
    heap.push(item);
    let index = heap.length - 1;
    while (index > 0) {
      const parent = Math.floor((index - 1) / 2);
      if (!isBefore(heap[index], heap[parent])) {
        break;
      }
      [heap[index], heap[parent]] = [heap[parent], heap[index]];
      index = parent;
    }
  };

  const popHeap = (): { listIndex: number; segment: TranscriptionSegment } => {
    const root = heap[0];
    if (!root) {
      throw new Error('Cannot pop an empty transcription heap');
    }
    const last = heap.pop();
    if (last && heap.length > 0) {
      heap[0] = last;
      let index = 0;
      while (true) {
        const left = index * 2 + 1;
        const right = left + 1;
        let smallest = index;

        if (left < heap.length && isBefore(heap[left], heap[smallest])) {
          smallest = left;
        }
        if (right < heap.length && isBefore(heap[right], heap[smallest])) {
          smallest = right;
        }
        if (smallest === index) {
          break;
        }

        [heap[index], heap[smallest]] = [heap[smallest], heap[index]];
        index = smallest;
      }
    }
    return root;
  };

  for (let listIndex = 0; listIndex < lists.length; listIndex += 1) {
    const segment = lists[listIndex][0];
    if (segment) {
      pushHeap({ listIndex, segment });
    }
  }

  while (heap.length > 0) {
    const item = popHeap();
    merged.push(item.segment);
    cursors[item.listIndex] += 1;

    const next = lists[item.listIndex][cursors[item.listIndex]];
    if (next) {
      pushHeap({ listIndex: item.listIndex, segment: next });
    }
  }

  return merged;
}

export async function transcribeChunksConcurrently({
  chunks,
  apiKey,
  concurrency,
  transcribeChunk,
  onChunkDone,
}: TranscriptionQueueOptions): Promise<TranscriptionSegment[]> {
  if (chunks.length === 0) {
    return [];
  }

  const requestedConcurrency =
    Number.isFinite(concurrency) && concurrency >= 1 ? Math.floor(concurrency) : 1;
  const safeConcurrency = Math.min(requestedConcurrency, chunks.length);
  const results = new Map<number, TranscriptionSegment[]>();
  const totalAudioSec = chunks.reduce((sum, chunk) => sum + chunk.durationSec, 0);
  let completedChunks = 0;
  let completedAudioSec = 0;
  let nextIndex = 0;
  let failed = false;

  async function worker() {
    while (!failed && nextIndex < chunks.length) {
      const chunk = chunks[nextIndex];
      nextIndex += 1;

      let segments: TranscriptionSegment[];
      try {
        segments = await transcribeChunk(chunk.audioPath, apiKey, chunk.offsetSec);
      } catch (error) {
        failed = true;
        throw error;
      }

      if (failed) {
        return;
      }

      results.set(chunk.index, segments);
      completedChunks += 1;
      completedAudioSec += chunk.durationSec;
      onChunkDone?.({
        chunkIndex: chunk.index,
        completedChunks,
        totalChunks: chunks.length,
        completedAudioSec,
        totalAudioSec,
      });
    }
  }

  await Promise.all(Array.from({ length: safeConcurrency }, () => worker()));

  const merged = mergeSegmentsByStart(chunks, results);
  for (let id = 0; id < merged.length; id += 1) {
    merged[id].id = id;
  }
  return merged;
}
