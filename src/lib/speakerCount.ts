const MIN_INFERRED_SPEAKERS = 2;
const MAX_INFERRED_SPEAKERS = 12;
const AUTO_CHUNKED_MIN_AUDIO_SEC = 30 * 60;
const AUTO_CHUNKED_MIN_CHUNKS = 6;

export type DiarizationPlanStrategy = "full-audio" | "chunked-known" | "chunked-auto";

export interface DiarizationPlanInput {
  expectedSpeakers?: number;
  audioChunkCount: number;
  totalAudioSec?: number;
}

export interface DiarizationPlan {
  strategy: DiarizationPlanStrategy;
  preferChunked: boolean;
  expectedSpeakers?: number;
  label: string;
  note: string;
  warning?: string;
}

const normalizeConfiguredExpectedSpeakers = (value?: number) => {
  if (value === undefined || !Number.isFinite(value)) return undefined;
  const count = Math.floor(value);
  return count >= MIN_INFERRED_SPEAKERS && count <= MAX_INFERRED_SPEAKERS ? count : undefined;
};

export const inferExpectedSpeakersFromParticipants = (participantNames: string[]) => {
  const uniqueNames = new Set(
    participantNames
      .map((name) => name.trim().toLocaleLowerCase("pt-BR"))
      .filter(Boolean),
  );
  const count = uniqueNames.size;
  return count >= MIN_INFERRED_SPEAKERS && count <= MAX_INFERRED_SPEAKERS
    ? count
    : undefined;
};

export const resolveDiarizationExpectedSpeakers = (
  configuredExpectedSpeakers: number | undefined,
  participantNames: string[],
) =>
  normalizeConfiguredExpectedSpeakers(configuredExpectedSpeakers) ??
  inferExpectedSpeakersFromParticipants(participantNames);

export const shouldPreferChunkedDiarization = (
  expectedSpeakers: number | undefined,
  audioChunkCount: number,
  totalAudioSec = 0,
) =>
  buildDiarizationPlan({
    expectedSpeakers,
    audioChunkCount,
    totalAudioSec,
  }).preferChunked;

export const buildDiarizationPlan = ({
  expectedSpeakers,
  audioChunkCount,
  totalAudioSec = 0,
}: DiarizationPlanInput): DiarizationPlan => {
  const normalizedExpectedSpeakers = normalizeConfiguredExpectedSpeakers(expectedSpeakers);
  const safeChunkCount = Number.isFinite(audioChunkCount) ? Math.max(0, Math.floor(audioChunkCount)) : 0;
  const safeTotalAudioSec = Number.isFinite(totalAudioSec) ? Math.max(0, totalAudioSec) : 0;
  const hasMultipleChunks = safeChunkCount > 1;

  if (normalizedExpectedSpeakers && hasMultipleChunks) {
    return {
      strategy: "chunked-known",
      preferChunked: true,
      expectedSpeakers: normalizedExpectedSpeakers,
      label: `${normalizedExpectedSpeakers} falantes esperados`,
      note: `CPU moderno em blocos com ${normalizedExpectedSpeakers} falantes esperados.`,
    };
  }

  if (
    hasMultipleChunks &&
    (safeTotalAudioSec >= AUTO_CHUNKED_MIN_AUDIO_SEC ||
      safeChunkCount >= AUTO_CHUNKED_MIN_CHUNKS)
  ) {
    return {
      strategy: "chunked-auto",
      preferChunked: true,
      label: "Automatico em blocos",
      note: "CPU moderno em blocos automatico; o numero de falantes sera detectado.",
      warning:
        "Informe o numero de falantes quando souber; isso melhora a estabilidade dos rotulos.",
    };
  }

  return {
    strategy: "full-audio",
    preferChunked: false,
    expectedSpeakers: normalizedExpectedSpeakers,
    label: normalizedExpectedSpeakers
      ? `${normalizedExpectedSpeakers} falantes esperados`
      : "Detectar automaticamente",
    note: normalizedExpectedSpeakers
      ? `CPU moderno inteiro com ${normalizedExpectedSpeakers} falantes esperados.`
      : "CPU moderno inteiro; adequado para audios curtos sem numero de falantes.",
  };
};
