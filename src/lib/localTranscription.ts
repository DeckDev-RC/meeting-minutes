import { transcribeChunksLocal, transcribeChunksParakeetLocal } from "./tauri";
import type { ExportedChunk, LocalTranscriptionChunkResult } from "./types";
import type { TranscriptionBackend } from "./transcriptionProvider";

export function transcribeChunksWithLocalBackend(
  backend: TranscriptionBackend,
  chunks: ExportedChunk[],
): Promise<LocalTranscriptionChunkResult[]> {
  if (backend === "parakeet-local") {
    return transcribeChunksParakeetLocal(chunks);
  }

  if (backend === "local") {
    return transcribeChunksLocal(chunks, "turbo");
  }

  throw new Error(`Backend ${backend} nao e local.`);
}
