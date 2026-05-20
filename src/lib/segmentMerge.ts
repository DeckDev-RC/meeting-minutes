import type { TranscriptionSegment } from "./types";

export function mergeSortedTranscriptionSegments(
  left: TranscriptionSegment[],
  right: TranscriptionSegment[],
): TranscriptionSegment[] {
  const merged: TranscriptionSegment[] = [];
  let leftIndex = 0;
  let rightIndex = 0;

  while (leftIndex < left.length || rightIndex < right.length) {
    const leftSegment = left[leftIndex];
    const rightSegment = right[rightIndex];

    if (
      rightSegment === undefined ||
      (leftSegment !== undefined && leftSegment.start <= rightSegment.start)
    ) {
      merged.push({ ...leftSegment, id: merged.length });
      leftIndex += 1;
    } else {
      merged.push({ ...rightSegment, id: merged.length });
      rightIndex += 1;
    }
  }

  return merged;
}
