import type { TranscriptionSegment } from "./types";

function appendWithSequentialId(merged: TranscriptionSegment[], segment: TranscriptionSegment) {
  segment.id = merged.length;
  merged.push(segment);
}

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
      appendWithSequentialId(merged, leftSegment);
      leftIndex += 1;
    } else {
      appendWithSequentialId(merged, rightSegment);
      rightIndex += 1;
    }
  }

  return merged;
}
