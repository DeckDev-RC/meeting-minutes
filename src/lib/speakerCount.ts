const MIN_INFERRED_SPEAKERS = 2;
const MAX_INFERRED_SPEAKERS = 8;

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
) => Boolean(expectedSpeakers && audioChunkCount > 1);
