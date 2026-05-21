import type { ProcessingChunkRecord, TranscriptionSegment } from "./types";

export function resolveSegmentsForFactScheduling(
  chunk: ProcessingChunkRecord,
  knownSegments: TranscriptionSegment[] | undefined,
  parseStoredSegments: (chunk: ProcessingChunkRecord) => TranscriptionSegment[],
) {
  if (chunk.status !== "done") return null;
  const segments = knownSegments ?? parseStoredSegments(chunk);
  return segments.length > 0 ? segments : null;
}
