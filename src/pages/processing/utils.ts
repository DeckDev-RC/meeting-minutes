import { sanitizeMeetingChunkInsights } from "../../lib/minutesEvidence";
import type {
  ExportedChunk,
  MeetingChunkInsights,
  ProcessingChunkRecord,
  ProcessingProfile,
  TranscriptionSegment,
} from "../../lib/types";

export const joinPath = (dir: string, fileName: string) => {
  const separator = dir.includes("\\") ? "\\" : "/";
  return `${dir.replace(/[\\/]+$/, "")}${separator}${fileName}`;
};

export const chunkOverlapForProfile = (profile: ProcessingProfile) => {
  if (profile === "turbo") return 0;
  return 3;
};

export const formatError = (err: unknown) => {
  if (typeof err === "string") return err.trim();
  if (err instanceof Error) return err.message.trim();
  if (err && typeof err === "object") {
    const maybeMessage =
      "message" in err ? String((err as { message?: unknown }).message ?? "") : "";
    if (maybeMessage.trim()) return maybeMessage.trim();
    try {
      return JSON.stringify(err);
    } catch {
      return String(err);
    }
  }
  return "";
};

export const toExportedChunk = (chunk: ProcessingChunkRecord): ExportedChunk => ({
  index: chunk.index,
  audioPath: chunk.audioPath,
  startSec: chunk.startSec,
  endSec: chunk.endSec,
  offsetSec: chunk.offsetSec,
  durationSec: chunk.durationSec,
});

export const toNewProcessingChunkRecord = (
  meetingId: string,
  chunk: ExportedChunk,
): ProcessingChunkRecord => ({
  meetingId,
  index: chunk.index,
  audioPath: chunk.audioPath,
  startSec: chunk.startSec,
  endSec: chunk.endSec,
  offsetSec: chunk.offsetSec,
  durationSec: chunk.durationSec,
  status: "pending",
  rawSegmentsJson: null,
  errorMsg: null,
  factsStatus: "pending",
  factsJson: null,
  factsErrorMsg: null,
});

export const sumChunkDurations = (chunks: Pick<ProcessingChunkRecord, "durationSec">[]) =>
  chunks.reduce((sum, chunk) => sum + chunk.durationSec, 0);

const isTranscriptionSegment = (value: unknown): value is TranscriptionSegment => {
  if (!value || typeof value !== "object") return false;
  const segment = value as Partial<TranscriptionSegment>;

  return (
    typeof segment.id === "number" &&
    typeof segment.start === "number" &&
    typeof segment.end === "number" &&
    typeof segment.text === "string"
  );
};

export const parseStoredSegments = (chunk: ProcessingChunkRecord): TranscriptionSegment[] => {
  if (!chunk.rawSegmentsJson) {
    throw new Error(`Trecho ${chunk.index} marcado como concluido sem transcricao salva.`);
  }

  try {
    const parsed = JSON.parse(chunk.rawSegmentsJson) as unknown;
    if (!Array.isArray(parsed) || !parsed.every(isTranscriptionSegment)) {
      throw new Error("formato inesperado");
    }
    return parsed;
  } catch (err) {
    throw new Error(
      `Transcricao salva do trecho ${chunk.index} esta invalida: ${
        formatError(err) || "JSON invalido"
      }`,
    );
  }
};

const isMeetingChunkInsights = (value: unknown): value is MeetingChunkInsights => {
  if (!value || typeof value !== "object") return false;
  const item = value as Partial<MeetingChunkInsights>;
  return (
    typeof item.chunkIndex === "number" &&
    typeof item.startSec === "number" &&
    typeof item.endSec === "number" &&
    typeof item.summary === "string" &&
    Array.isArray(item.topics) &&
    Array.isArray(item.decisions) &&
    Array.isArray(item.actions) &&
    Array.isArray(item.questions) &&
    Array.isArray(item.risks)
  );
};

export const parseCachedFacts = (chunk: ProcessingChunkRecord): MeetingChunkInsights | null => {
  if (chunk.factsStatus !== "done" || !chunk.factsJson) return null;
  try {
    const parsed = JSON.parse(chunk.factsJson) as unknown;
    return isMeetingChunkInsights(parsed) ? sanitizeMeetingChunkInsights(parsed) : null;
  } catch {
    return null;
  }
};

export const summarizeBackendError = (message: string) => {
  const clean = message.replace(/\r/g, "").trim();
  if (!clean) return "";
  if (clean.includes("Please enable access to public gated repositories")) {
    return "Token Hugging Face sem permissao para repositorios publicos gated.";
  }
  if (clean.includes("403 Forbidden")) {
    return "Hugging Face retornou 403 para o modelo pyannote.";
  }
  if (clean.includes("HF_TOKEN is not available")) {
    return "HF_TOKEN nao esta disponivel para o worker pyannote.";
  }
  if (clean.includes("pyannote.audio") && clean.includes("not installed")) {
    return "Ambiente pyannote nao esta instalado.";
  }
  return clean.split("\n").find((line) => line.trim())?.trim().slice(0, 180) || "";
};

export const normalizeProcessingProfile = (
  value: string | null | undefined,
): ProcessingProfile => {
  if (value === "turbo" || value === "precision") return value;
  return "balanced";
};

export const parseParticipantsHint = (hint: string | null | undefined) =>
  (hint || "")
    .split(/[\n,;]+/)
    .map((name) => name.trim())
    .filter(Boolean)
    .slice(0, 30);

export const htmlToReadablePreview = (html: string) =>
  html
    .replace(/<style[\s\S]*?<\/style>/gi, " ")
    .replace(/<script[\s\S]*?<\/script>/gi, " ")
    .replace(/<\/(p|div|section|h[1-6]|li|tr)>/gi, "\n")
    .replace(/<[^>]+>/g, " ")
    .replace(/&nbsp;/g, " ")
    .replace(/&amp;/g, "&")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/[ \t]+/g, " ")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
