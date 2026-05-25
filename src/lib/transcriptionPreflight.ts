import type { TranscriptionRoutingProfile } from "./types";
import {
  selectFallbackTranscriptionBackends,
  selectTranscriptionBackend,
  transcriptionBackendLabel,
  type TranscriptionBackend,
} from "./transcriptionProvider";

export type TranscriptionBudgetProfile = "free-local" | "low-cost" | "max-quality";

export interface MoneyEstimate {
  amount: number;
  currency: "USD";
}

export interface QuotaRisk {
  level: "none" | "low" | "medium" | "high" | "blocked";
  label: string;
  detail: string;
}

export interface TranscriptionPreflightInput {
  durationSec: number;
  budgetProfile: TranscriptionBudgetProfile;
  groqApiKey?: string | null;
  cloudflareAccountId?: string | null;
  cloudflareApiToken?: string | null;
  deepgramApiKey?: string | null;
  localBackendAvailable?: boolean;
  parakeetBackendAvailable?: boolean;
  manualProvider?: TranscriptionBackend | null;
  cloudflareQuotaExhaustedToday?: boolean;
}

export interface TranscriptionPreflight {
  durationSec: number;
  budgetProfile: TranscriptionBudgetProfile;
  transcriptionProfile: TranscriptionRoutingProfile;
  backend: TranscriptionBackend;
  backendLabel: string;
  fallbackBackends: TranscriptionBackend[];
  fallbackLabel: string;
  quotaRisk: QuotaRisk;
  deepgramFallbackCost: MoneyEstimate;
}

const DEEPGRAM_USD_PER_MIN = 0.0071;

export function mapBudgetToTranscriptionProfile(
  budgetProfile: TranscriptionBudgetProfile,
): TranscriptionRoutingProfile {
  if (budgetProfile === "free-local") return "offline-free";
  if (budgetProfile === "max-quality") return "max-quality";
  return "smart-low-cost";
}

export function estimateDeepgramCost(durationSec: number): MoneyEstimate {
  const safeDurationSec = Number.isFinite(durationSec) && durationSec > 0 ? durationSec : 0;
  return {
    amount: Number(((safeDurationSec / 60) * DEEPGRAM_USD_PER_MIN).toFixed(2)),
    currency: "USD",
  };
}

function buildQuotaRisk(
  backend: TranscriptionBackend,
  durationSec: number,
  cloudflareQuotaExhaustedToday: boolean,
): QuotaRisk {
  if (cloudflareQuotaExhaustedToday) {
    return {
      level: "blocked",
      label: "Cloudflare esgotado hoje",
      detail: "A cota gratuita diaria ja falhou hoje; o app pula Cloudflare ate amanha.",
    };
  }

  if (backend !== "cloudflare") {
    return {
      level: "none",
      label: "Sem risco de cota Cloudflare",
      detail: "A rota escolhida nao depende do Cloudflare nesta reuniao.",
    };
  }

  if (durationSec >= 3 * 3600) {
    return {
      level: "high",
      label: "Risco alto de cota",
      detail: "Reunioes longas podem consumir a alocacao diaria gratuita do Cloudflare.",
    };
  }

  if (durationSec >= 90 * 60) {
    return {
      level: "medium",
      label: "Risco medio de cota",
      detail: "A reuniao e longa; mantenha fallback configurado antes de iniciar.",
    };
  }

  return {
    level: "low",
    label: "Risco baixo de cota",
    detail: "Duracao dentro do uso normal do perfil economico.",
  };
}

export function buildTranscriptionPreflight({
  durationSec,
  budgetProfile,
  groqApiKey,
  cloudflareAccountId,
  cloudflareApiToken,
  deepgramApiKey,
  localBackendAvailable,
  parakeetBackendAvailable,
  manualProvider,
  cloudflareQuotaExhaustedToday = false,
}: TranscriptionPreflightInput): TranscriptionPreflight {
  const transcriptionProfile = mapBudgetToTranscriptionProfile(budgetProfile);
  const unavailableBackends: TranscriptionBackend[] = cloudflareQuotaExhaustedToday
    ? ["cloudflare"]
    : [];
  const backend = selectTranscriptionBackend({
    totalAudioSec: durationSec,
    groqApiKey,
    cloudflareAccountId,
    cloudflareApiToken,
    deepgramApiKey,
    localBackendAvailable,
    parakeetBackendAvailable,
    profile: transcriptionProfile,
    manualProvider,
    unavailableBackends,
  });
  const fallbackBackends =
    budgetProfile === "free-local"
      ? []
      : selectFallbackTranscriptionBackends({
          totalAudioSec: durationSec,
          groqApiKey,
          cloudflareAccountId,
          cloudflareApiToken,
          deepgramApiKey,
          localBackendAvailable,
          parakeetBackendAvailable,
          profile: transcriptionProfile,
          manualProvider,
          primaryBackend: backend,
          unavailableBackends,
        });
  const fallbackLabel =
    fallbackBackends.length > 0
      ? fallbackBackends.map(transcriptionBackendLabel).join(" -> ")
      : "Sem fallback automatico";

  return {
    durationSec,
    budgetProfile,
    transcriptionProfile,
    backend,
    backendLabel: transcriptionBackendLabel(backend),
    fallbackBackends,
    fallbackLabel,
    quotaRisk: buildQuotaRisk(backend, durationSec, cloudflareQuotaExhaustedToday),
    deepgramFallbackCost:
      budgetProfile === "free-local" ? { amount: 0, currency: "USD" } : estimateDeepgramCost(durationSec),
  };
}
