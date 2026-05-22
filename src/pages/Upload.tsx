import { useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import DropZone from "../components/DropZone";
import { getApiKeys, probeMediaMetadata, saveMeeting } from "../lib/tauri";
import {
  getCloudflareQuotaState,
  type CloudflareQuotaState,
} from "../lib/cloudTranscriptionHealth";
import {
  buildTranscriptionPreflight,
  mapBudgetToTranscriptionProfile,
  type TranscriptionBudgetProfile,
  type TranscriptionPreflight,
} from "../lib/transcriptionPreflight";
import {
  buildDiarizationPlan,
  resolveDiarizationExpectedSpeakers,
} from "../lib/speakerCount";
import type { ProcessingProfile } from "../lib/types";
import { useMeetingStore } from "../store/meetingStore";

const PROFILE_OPTIONS: {
  value: ProcessingProfile;
  label: string;
  detail: string;
}[] = [
  {
    value: "turbo",
    label: "Turbo",
    detail: "Menos revisao local. Prioriza tempo final.",
  },
  {
    value: "balanced",
    label: "Equilibrado",
    detail: "Padrao recomendado para velocidade e estabilidade.",
  },
  {
    value: "precision",
    label: "Precisao",
    detail: "Revisa mais trechos suspeitos. Leva mais tempo.",
  },
];

const BUDGET_OPTIONS: {
  value: TranscriptionBudgetProfile;
  label: string;
  detail: string;
}[] = [
  {
    value: "low-cost",
    label: "Baixo custo",
    detail: "Cloudflare primeiro; Deepgram entra se houver cota/qualidade em risco.",
  },
  {
    value: "free-local",
    label: "R$ 0 offline",
    detail: "Usa backend local e evita API de transcricao.",
  },
  {
    value: "max-quality",
    label: "Qualidade maxima",
    detail: "Deepgram direto para nomes e termos mais estaveis.",
  },
];

const formatDuration = (durationSec: number) => {
  if (!Number.isFinite(durationSec) || durationSec <= 0) return "Nao detectada";
  const totalMinutes = Math.round(durationSec / 60);
  const hours = Math.floor(totalMinutes / 60);
  const minutes = totalMinutes % 60;
  if (hours <= 0) return `${minutes} min`;
  return `${hours}h ${String(minutes).padStart(2, "0")}min`;
};

const formatUsd = (amount: number) => `US$ ${amount.toFixed(2)}`;

const parseParticipantsHint = (hint: string) =>
  hint
    .split(/[\n,;]+/)
    .map((name) => name.trim())
    .filter(Boolean)
    .slice(0, 30);

export default function Upload() {
  const navigate = useNavigate();
  const [filePath, setFilePath] = useState<string | null>(null);
  const [processingProfile, setProcessingProfile] = useState<ProcessingProfile>("balanced");
  const [budgetProfile, setBudgetProfile] = useState<TranscriptionBudgetProfile>("low-cost");
  const [participantsHint, setParticipantsHint] = useState("");
  const [loading, setLoading] = useState(false);
  const [durationSec, setDurationSec] = useState<number | null>(null);
  const [preflightLoading, setPreflightLoading] = useState(false);
  const [preflightError, setPreflightError] = useState("");
  const [apiKeys, setApiKeysState] = useState<Awaited<ReturnType<typeof getApiKeys>> | null>(null);
  const [cloudflareQuotaState, setCloudflareQuotaState] =
    useState<CloudflareQuotaState>(() =>
      getCloudflareQuotaState(typeof window === "undefined" ? undefined : window.localStorage),
    );
  const { setCurrentMeeting, reset } = useMeetingStore();

  useEffect(() => {
    let cancelled = false;
    setCloudflareQuotaState(
      getCloudflareQuotaState(typeof window === "undefined" ? undefined : window.localStorage),
    );
    getApiKeys()
      .then((keys) => {
        if (cancelled) return;
        setApiKeysState(keys);
        if (keys.transcriptionProfile === "offline-free") setBudgetProfile("free-local");
        if (keys.transcriptionProfile === "max-quality") setBudgetProfile("max-quality");
      })
      .catch(() => {
        if (!cancelled) setApiKeysState(null);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (!filePath) {
      setDurationSec(null);
      setPreflightError("");
      return;
    }

    let cancelled = false;
    setPreflightLoading(true);
    setPreflightError("");
    probeMediaMetadata(filePath)
      .then((metadata) => {
        if (cancelled) return;
        setDurationSec(metadata.durationSec ?? null);
      })
      .catch((err) => {
        if (cancelled) return;
        console.error(err);
        setDurationSec(null);
        setPreflightError("Nao foi possivel detectar a duracao antes de iniciar.");
      })
      .finally(() => {
        if (!cancelled) setPreflightLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [filePath]);

  const preflight: TranscriptionPreflight | null = useMemo(() => {
    if (!apiKeys || !durationSec) return null;
    return buildTranscriptionPreflight({
      durationSec,
      budgetProfile,
      groqApiKey: apiKeys.groq,
      cloudflareAccountId: apiKeys.cloudflareAccountId,
      cloudflareApiToken: apiKeys.cloudflareApiToken,
      deepgramApiKey: apiKeys.deepgramApiKey,
      manualProvider: apiKeys.manualTranscriptionProvider,
      cloudflareQuotaExhaustedToday: cloudflareQuotaState.isExhaustedToday,
    });
  }, [apiKeys, budgetProfile, cloudflareQuotaState.isExhaustedToday, durationSec]);

  const speakerPlan = useMemo(() => {
    if (!apiKeys || !durationSec) return null;
    const participantNames = parseParticipantsHint(participantsHint);
    const expectedSpeakers = resolveDiarizationExpectedSpeakers(
      apiKeys.expectedSpeakers,
      participantNames,
    );
    return buildDiarizationPlan({
      expectedSpeakers,
      audioChunkCount: Math.max(1, Math.ceil(durationSec / 360)),
      totalAudioSec: durationSec,
    });
  }, [apiKeys, durationSec, participantsHint]);

  const handleProcess = async () => {
    if (!filePath) return;
    setLoading(true);
    try {
      reset();
      const id = await saveMeeting({
        filePath,
        participantsHint: participantsHint.trim() || null,
        processingProfile,
        transcriptionProfile:
          preflight?.transcriptionProfile ?? mapBudgetToTranscriptionProfile(budgetProfile),
        status: "processing",
      });
      setCurrentMeeting(id);
      navigate(`/processing/${id}`);
    } catch (err) {
      console.error(err);
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="mx-auto max-w-5xl space-y-6">
      <header>
        <h2 className="text-2xl font-bold text-gray-950">Nova reuniao</h2>
        <p className="mt-2 max-w-2xl text-sm leading-6 text-gray-600">
          Envie um audio ou video. O app prepara o arquivo, transcreve em blocos e monta a ata.
        </p>
      </header>

      <div className="grid gap-6 lg:grid-cols-[1fr_320px]">
        <section className="rounded-xl border border-gray-200 bg-white p-4 shadow-sm md:p-6">
          <DropZone onFileSelected={setFilePath} />

          <div className="mt-6 grid gap-4 border-t border-gray-100 pt-5">
            <div>
              <label className="mb-2 block text-sm font-semibold text-gray-900">
                Perfil de processamento
              </label>
              <div className="grid gap-2 sm:grid-cols-3" role="radiogroup">
                {PROFILE_OPTIONS.map((option) => {
                  const selected = processingProfile === option.value;
                  return (
                    <button
                      key={option.value}
                      type="button"
                      onClick={() => setProcessingProfile(option.value)}
                      className={`rounded-lg border px-3 py-3 text-left transition-colors ${
                        selected
                          ? "border-blue-500 bg-blue-50 text-blue-900 ring-2 ring-blue-100"
                          : "border-gray-200 bg-white text-gray-700 hover:border-blue-200 hover:bg-blue-50/40"
                      }`}
                      role="radio"
                      aria-checked={selected}
                    >
                      <span className="block text-sm font-semibold">{option.label}</span>
                      <span className="mt-1 block text-xs leading-5 text-gray-500">
                        {option.detail}
                      </span>
                    </button>
                  );
                })}
              </div>
            </div>

            <div>
              <label className="mb-2 block text-sm font-semibold text-gray-900">
                Orcamento da transcricao
              </label>
              <div className="grid gap-2 sm:grid-cols-3" role="radiogroup">
                {BUDGET_OPTIONS.map((option) => {
                  const selected = budgetProfile === option.value;
                  return (
                    <button
                      key={option.value}
                      type="button"
                      onClick={() => setBudgetProfile(option.value)}
                      className={`rounded-lg border px-3 py-3 text-left transition-colors ${
                        selected
                          ? "border-blue-500 bg-blue-50 text-blue-900 ring-2 ring-blue-100"
                          : "border-gray-200 bg-white text-gray-700 hover:border-blue-200 hover:bg-blue-50/40"
                      }`}
                      role="radio"
                      aria-checked={selected}
                    >
                      <span className="block text-sm font-semibold">{option.label}</span>
                      <span className="mt-1 block text-xs leading-5 text-gray-500">
                        {option.detail}
                      </span>
                    </button>
                  );
                })}
              </div>
            </div>

            <div>
              <label className="mb-1.5 block text-sm font-semibold text-gray-900">
                Participantes conhecidos
              </label>
              <textarea
                value={participantsHint}
                onChange={(event) => setParticipantsHint(event.target.value)}
                rows={3}
                placeholder="Caio, Emanuella, Ana Paula"
                className="w-full resize-none rounded-lg border border-gray-300 px-3 py-2.5 text-sm shadow-sm focus:border-blue-500 focus:ring-2 focus:ring-blue-100"
              />
              <p className="mt-1.5 text-xs leading-5 text-gray-500">
                Opcional. Use nomes separados por virgula ou linha.
              </p>
            </div>
          </div>

          {filePath && (
            <div className="mt-6 space-y-4 border-t border-gray-100 pt-5">
              <div className="rounded-lg border border-gray-200 bg-gray-50 px-4 py-3">
                <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                  <div>
                    <p className="text-sm font-semibold text-gray-900">Pre-check da reuniao</p>
                    <p className="mt-1 text-xs leading-5 text-gray-500">
                      {preflightLoading
                        ? "Detectando duracao e rota de transcricao..."
                        : preflightError || "Revise custo, cota e fallback antes de iniciar."}
                    </p>
                  </div>
                  <span
                    className={`w-fit rounded-full px-2.5 py-1 text-xs font-semibold ${
                      preflight?.quotaRisk.level === "high" ||
                      preflight?.quotaRisk.level === "blocked"
                        ? "bg-amber-100 text-amber-800"
                        : "bg-emerald-100 text-emerald-700"
                    }`}
                  >
                    {preflight?.quotaRisk.label ?? "Calculando"}
                  </span>
                </div>
                <dl className="mt-4 grid gap-3 text-sm sm:grid-cols-2 lg:grid-cols-6">
                  <div>
                    <dt className="text-xs font-medium uppercase tracking-wide text-gray-500">
                      Duracao
                    </dt>
                    <dd className="mt-1 font-semibold text-gray-900">
                      {durationSec ? formatDuration(durationSec) : "Calculando"}
                    </dd>
                  </div>
                  <div>
                    <dt className="text-xs font-medium uppercase tracking-wide text-gray-500">
                      Backend
                    </dt>
                    <dd className="mt-1 font-semibold text-gray-900">
                      {preflight?.backendLabel ?? "Calculando"}
                    </dd>
                  </div>
                  <div>
                    <dt className="text-xs font-medium uppercase tracking-wide text-gray-500">
                      Fallback
                    </dt>
                    <dd className="mt-1 font-semibold text-gray-900">
                      {preflight?.fallbackLabel ?? "Calculando"}
                    </dd>
                  </div>
                  <div>
                    <dt className="text-xs font-medium uppercase tracking-wide text-gray-500">
                      Cota
                    </dt>
                    <dd className="mt-1 font-semibold text-gray-900">
                      {preflight?.quotaRisk.detail ?? "Aguardando duracao"}
                    </dd>
                  </div>
                  <div>
                    <dt className="text-xs font-medium uppercase tracking-wide text-gray-500">
                      Deepgram
                    </dt>
                    <dd className="mt-1 font-semibold text-gray-900">
                      {preflight
                        ? `${formatUsd(preflight.deepgramFallbackCost.amount)} se usado`
                        : "Calculando"}
                    </dd>
                  </div>
                  <div>
                    <dt className="text-xs font-medium uppercase tracking-wide text-gray-500">
                      Falantes
                    </dt>
                    <dd className="mt-1 font-semibold text-gray-900">
                      {speakerPlan?.label ?? "Calculando"}
                    </dd>
                  </div>
                </dl>
                {speakerPlan?.warning && (
                  <p className="mt-3 text-xs leading-5 text-amber-700">
                    {speakerPlan.warning}
                  </p>
                )}
              </div>
              <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                <p className="text-sm text-gray-500">Tudo certo para iniciar o processamento.</p>
                <button
                  onClick={handleProcess}
                  disabled={loading || preflightLoading}
                  className="rounded-lg bg-blue-600 px-6 py-3 text-sm font-semibold text-white shadow-sm transition-colors hover:bg-blue-700 disabled:cursor-not-allowed disabled:opacity-50"
                >
                  {loading ? "Iniciando..." : "Processar reuniao"}
                </button>
              </div>
            </div>
          )}
        </section>

        <aside className="rounded-xl border border-gray-200 bg-white p-5 shadow-sm">
          <h3 className="text-sm font-semibold text-gray-950">O que acontece depois</h3>
          <ol className="mt-4 space-y-4">
            {[
              ["1", "Audio limpo", "Converte para FLAC mono em 16 kHz."],
              ["2", "Blocos inteligentes", "Corta perto de pausas para preservar contexto."],
              ["3", "Transcricao paralela", "Processa varios blocos ao mesmo tempo."],
              ["4", "Ata final", "Organiza falantes e gera o documento."],
            ].map(([step, title, detail]) => (
              <li key={step} className="flex gap-3">
                <span className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-gray-950 text-xs font-semibold text-white">
                  {step}
                </span>
                <div>
                  <p className="text-sm font-semibold text-gray-900">{title}</p>
                  <p className="mt-1 text-xs leading-5 text-gray-500">{detail}</p>
                </div>
              </li>
            ))}
          </ol>
        </aside>
      </div>
    </div>
  );
}
