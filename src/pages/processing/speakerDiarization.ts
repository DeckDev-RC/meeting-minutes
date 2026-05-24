import {
  diarizeAudioTurnsModernCpu,
  diarizeAudioTurnsModernCpuChunked,
  diarizeAudioTurnsPyannote,
  diarizeAudioTurnsSherpaChunked,
} from "../../lib/tauri";
import {
  sherpaProviderForRuntime,
  shouldTrySherpaRuntime,
} from "../../lib/diarizationRuntime";
import type {
  ExportedChunk,
  SpeakerDiarizationRuntime,
  SpeakerTurn,
} from "../../lib/types";
import { formatError, summarizeBackendError } from "./utils";

const MAX_CHUNKED_DIARIZATION_WORKERS = 6;

export type SpeculativeSpeakerTurns = {
  turns: SpeakerTurn[];
  error: string;
  engine: "pyannote" | "modern-cpu" | "modern-cpu-chunked" | "sherpa-onnx" | "none";
  fallbackReason?: string;
};

export const startSpeculativeSpeakerTurns = (
  audioPath: string,
  expectedSpeakers?: number,
  audioChunks: ExportedChunk[] = [],
  preferChunked = false,
  preferPyannote = false,
  speakerRuntime: SpeakerDiarizationRuntime = "modern-cpu",
): Promise<SpeculativeSpeakerTurns> => {
  const runModernCpu = (fallbackReason?: string): Promise<SpeculativeSpeakerTurns> => {
    if (!preferChunked || audioChunks.length <= 1) {
      return diarizeAudioTurnsModernCpu(audioPath, expectedSpeakers)
        .then((turns) => ({
          turns,
          error: "",
          engine: "modern-cpu" as const,
          fallbackReason,
        }))
        .catch((err) => ({
          turns: [],
          engine: "none" as const,
          error: formatError(err) || "Diarizacao local indisponivel.",
          fallbackReason,
        }));
    }

    const logicalCores =
      typeof navigator === "undefined" ? 8 : navigator.hardwareConcurrency || 8;
    const recommendedWorkers = Math.min(
      MAX_CHUNKED_DIARIZATION_WORKERS,
      Math.max(3, Math.floor(logicalCores / 3)),
    );
    const workerCount = Math.min(recommendedWorkers, audioChunks.length);
    return diarizeAudioTurnsModernCpuChunked(audioChunks, expectedSpeakers, workerCount)
      .then((turns) => ({
        turns,
        error: "",
        engine: "modern-cpu-chunked" as const,
        fallbackReason,
      }))
      .catch((chunkedErr) =>
        diarizeAudioTurnsModernCpu(audioPath, expectedSpeakers)
          .then((turns) => ({
            turns,
            error: "",
            engine: "modern-cpu" as const,
            fallbackReason:
              fallbackReason || summarizeBackendError(formatError(chunkedErr)),
          }))
          .catch((err) => ({
            turns: [],
            engine: "none" as const,
            error: formatError(err) || "Diarizacao local indisponivel.",
            fallbackReason:
              fallbackReason || summarizeBackendError(formatError(chunkedErr)),
          })),
      );
  };

  if (shouldTrySherpaRuntime(speakerRuntime, audioChunks.length)) {
    const logicalCores =
      typeof navigator === "undefined" ? 8 : navigator.hardwareConcurrency || 8;
    const recommendedWorkers = Math.min(
      MAX_CHUNKED_DIARIZATION_WORKERS,
      Math.max(2, Math.floor(logicalCores / 3)),
    );
    const workerCount = Math.min(recommendedWorkers, audioChunks.length);
    return diarizeAudioTurnsSherpaChunked(
      audioChunks,
      expectedSpeakers,
      workerCount,
      sherpaProviderForRuntime(speakerRuntime),
    )
      .then((turns) => ({ turns, error: "", engine: "sherpa-onnx" as const }))
      .catch((sherpaErr) =>
        runModernCpu(summarizeBackendError(formatError(sherpaErr))),
      );
  }

  if (preferPyannote) {
    return diarizeAudioTurnsPyannote(audioPath, expectedSpeakers)
      .then((turns) => ({ turns, error: "", engine: "pyannote" as const }))
      .catch((pyannoteErr) =>
        diarizeAudioTurnsModernCpu(audioPath, expectedSpeakers)
          .then((turns) => ({
            turns,
            error: "",
            engine: "modern-cpu" as const,
            fallbackReason: summarizeBackendError(formatError(pyannoteErr)),
          }))
          .catch((err) => ({
            turns: [],
            engine: "none" as const,
            error: formatError(err) || "Diarizacao local indisponivel.",
            fallbackReason: summarizeBackendError(formatError(pyannoteErr)),
          })),
      );
  }

  return runModernCpu();
};
