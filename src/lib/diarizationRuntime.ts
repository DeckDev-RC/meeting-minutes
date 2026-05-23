import type { SpeakerDiarizationRuntime } from "./types";

export const normalizeSpeakerDiarizationRuntime = (
  value: string | null | undefined,
): SpeakerDiarizationRuntime => {
  if (value === "sherpa-onnx-cpu" || value === "sherpa-onnx-cuda") return value;
  return "modern-cpu";
};

export const sherpaProviderForRuntime = (
  runtime: SpeakerDiarizationRuntime,
): "cpu" | "cuda" | undefined => {
  if (runtime === "sherpa-onnx-cpu") return "cpu";
  if (runtime === "sherpa-onnx-cuda") return "cuda";
  return undefined;
};

export const shouldTrySherpaRuntime = (
  runtime: SpeakerDiarizationRuntime,
  audioChunkCount: number,
) => runtime !== "modern-cpu" && audioChunkCount > 1;

export const chunkOutputFormatForSpeakerRuntime = (
  runtime: SpeakerDiarizationRuntime,
) => (runtime === "modern-cpu" ? "flac" : "wav");
