import { transcribeChunk } from './tauri';
import type { TranscriptionSegment } from './types';

export async function transcribeFile(
  chunks: string[],
  apiKey: string,
  chunkDuration: number,
  onProgress: (step: number, total: number) => void
): Promise<TranscriptionSegment[]> {
  const allSegments: TranscriptionSegment[] = [];

  for (let i = 0; i < chunks.length; i++) {
    onProgress(i + 1, chunks.length);
    const offset = i * chunkDuration;
    const segments = await transcribeChunk(chunks[i], apiKey, offset);
    allSegments.push(...segments);
  }

  return allSegments;
}
