import type { TranscriptionRoutingProfile } from "./types";

export type TranscriptionBackend =
  | "groq"
  | "cloudflare"
  | "deepgram"
  | "local"
  | "parakeet-local";

export const LOCAL_TRANSCRIPTION_REQUIRED_AUDIO_SEC = 7200;
export const DEFAULT_TRANSCRIPTION_PROFILE: TranscriptionRoutingProfile = "smart-low-cost";

export interface TranscriptionBackendSelectionInput {
  totalAudioSec: number;
  groqApiKey?: string | null;
  cloudflareAccountId?: string | null;
  cloudflareApiToken?: string | null;
  deepgramApiKey?: string | null;
  localBackendAvailable?: boolean;
  parakeetBackendAvailable?: boolean;
  profile?: TranscriptionRoutingProfile | null;
  manualProvider?: TranscriptionBackend | null;
  unavailableBackends?: TranscriptionBackend[];
}

function hasValue(value?: string | null) {
  return Boolean(value?.trim());
}

function localFallbackForDuration(totalAudioSec: number): TranscriptionBackend {
  return Number.isFinite(totalAudioSec) && totalAudioSec >= LOCAL_TRANSCRIPTION_REQUIRED_AUDIO_SEC
    ? "parakeet-local"
    : "local";
}

function isBackendConfigured(
  backend: TranscriptionBackend,
  input: TranscriptionBackendSelectionInput,
) {
  if (backend === "cloudflare") {
    return hasValue(input.cloudflareAccountId) && hasValue(input.cloudflareApiToken);
  }
  if (backend === "deepgram") {
    return hasValue(input.deepgramApiKey);
  }
  if (backend === "groq") {
    return hasValue(input.groqApiKey);
  }
  if (backend === "parakeet-local") {
    return input.parakeetBackendAvailable ?? true;
  }
  if (backend === "local") {
    return input.localBackendAvailable ?? true;
  }
  return true;
}

function firstConfigured(
  input: TranscriptionBackendSelectionInput,
  candidates: TranscriptionBackend[],
) {
  const unavailable = new Set(input.unavailableBackends ?? []);
  return candidates.find((backend) => !unavailable.has(backend) && isBackendConfigured(backend, input));
}

export function isQuotaOrRateLimitError(message: string) {
  const normalized = message.toLowerCase();
  return (
    normalized.includes("429") ||
    normalized.includes("too many requests") ||
    normalized.includes("rate limit") ||
    normalized.includes("rate_limit") ||
    normalized.includes("quota") ||
    normalized.includes("daily free allocation") ||
    normalized.includes("used up") ||
    normalized.includes("neurons") ||
    normalized.includes("limit exceeded")
  );
}

export function selectFallbackTranscriptionBackends({
  primaryBackend,
  unavailableBackends = [],
  ...input
}: TranscriptionBackendSelectionInput & {
  primaryBackend: TranscriptionBackend;
  unavailableBackends?: TranscriptionBackend[];
}): TranscriptionBackend[] {
  if (isLocalTranscriptionBackend(primaryBackend)) {
    return [];
  }

  const unavailable = new Set<TranscriptionBackend>([primaryBackend, ...unavailableBackends]);
  const candidatesByPrimary: Record<"groq" | "cloudflare" | "deepgram", TranscriptionBackend[]> = {
    cloudflare: ["deepgram", "groq", "local"],
    deepgram: ["cloudflare", "groq", "local"],
    groq: ["cloudflare", "deepgram", "local"],
  };

  return candidatesByPrimary[primaryBackend]
    .filter((backend) => !unavailable.has(backend))
    .filter((backend) => isBackendConfigured(backend, input));
}

export function selectTranscriptionBackend({
  totalAudioSec,
  groqApiKey,
  cloudflareAccountId,
  cloudflareApiToken,
  deepgramApiKey,
  localBackendAvailable,
  parakeetBackendAvailable,
  profile,
  manualProvider,
  unavailableBackends,
}: TranscriptionBackendSelectionInput): TranscriptionBackend {
  const input = {
    totalAudioSec,
    groqApiKey,
    cloudflareAccountId,
    cloudflareApiToken,
    deepgramApiKey,
    localBackendAvailable,
    parakeetBackendAvailable,
    profile,
    manualProvider,
    unavailableBackends,
  };
  const fallback = localFallbackForDuration(totalAudioSec);
  const activeProfile = profile || DEFAULT_TRANSCRIPTION_PROFILE;
  const unavailable = new Set(unavailableBackends ?? []);

  if (activeProfile === "manual" && manualProvider) {
    return !unavailable.has(manualProvider) && isBackendConfigured(manualProvider, input)
      ? manualProvider
      : fallback;
  }

  if (activeProfile === "offline-free") {
    return "parakeet-local";
  }

  if (activeProfile === "groq-turbo") {
    return firstConfigured(input, ["groq", "cloudflare", "deepgram"]) ?? fallback;
  }

  if (activeProfile === "max-quality") {
    return firstConfigured(input, ["deepgram", "cloudflare", "groq"]) ?? fallback;
  }

  if (
    !profile &&
    Number.isFinite(totalAudioSec) &&
    totalAudioSec >= LOCAL_TRANSCRIPTION_REQUIRED_AUDIO_SEC &&
    !hasValue(cloudflareAccountId) &&
    !hasValue(cloudflareApiToken) &&
    !hasValue(deepgramApiKey)
  ) {
    return "parakeet-local";
  }

  return firstConfigured(input, ["cloudflare", "deepgram", "groq"]) ?? fallback;
}

export function transcriptionBackendLabel(backend: TranscriptionBackend) {
  if (backend === "parakeet-local") {
    return "Parakeet local";
  }
  if (backend === "local") {
    return "faster-whisper local";
  }
  if (backend === "cloudflare") {
    return "Cloudflare Whisper";
  }
  if (backend === "deepgram") {
    return "Deepgram Nova-3";
  }
  return "Groq Whisper";
}

export function isLocalTranscriptionBackend(backend: TranscriptionBackend) {
  return backend === "local" || backend === "parakeet-local";
}
