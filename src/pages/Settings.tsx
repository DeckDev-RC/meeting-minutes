import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  checkLocalTranscriptionBackends,
  exportDiagnostics,
  getApiKeys,
  getOfflineTranscriptionRuntimeStatus,
  installOfflineTranscriptionRuntime,
  removeOfflineTranscriptionRuntime,
  setApiKeys,
  validateApiKeys,
  type ApiValidationResult,
  type OfflineTranscriptionRuntimeStatus,
} from "../lib/tauri";
import {
  clearCloudflareQuotaExhausted,
  getCloudflareQuotaState,
  type CloudflareQuotaState,
} from "../lib/cloudTranscriptionHealth";
import type { TranscriptionBackend } from "../lib/transcriptionProvider";
import type { TranscriptionRoutingProfile } from "../lib/types";

const DEFAULT_OFFLINE_RUNTIME_URL =
  "https://github.com/DeckDev-RC/meeting-minutes/releases/latest/download/meeting-minutes-transcribe-runtime-windows-x64.zip";

function formatBytes(value: number) {
  if (!Number.isFinite(value) || value <= 0) {
    return "0 MB";
  }
  const units = ["B", "KB", "MB", "GB"];
  let size = value;
  let unit = 0;
  while (size >= 1024 && unit < units.length - 1) {
    size /= 1024;
    unit += 1;
  }
  return `${size.toFixed(unit === 0 ? 0 : 1)} ${units[unit]}`;
}

export default function Settings() {
  const [groq, setGroq] = useState("");
  const [gemini, setGemini] = useState("");
  const [cloudflareAccountId, setCloudflareAccountId] = useState("");
  const [cloudflareApiToken, setCloudflareApiToken] = useState("");
  const [deepgramApiKey, setDeepgramApiKey] = useState("");
  const [transcriptionProfile, setTranscriptionProfile] =
    useState<TranscriptionRoutingProfile>("smart-low-cost");
  const [manualTranscriptionProvider, setManualTranscriptionProvider] =
    useState<TranscriptionBackend>("groq");
  const [expectedSpeakers, setExpectedSpeakers] = useState("");
  const [localStatus, setLocalStatus] = useState<{
    fasterWhisperAvailable: boolean;
    parakeetAvailable: boolean;
  } | null>(null);
  const [offlineRuntimeStatus, setOfflineRuntimeStatus] =
    useState<OfflineTranscriptionRuntimeStatus | null>(null);
  const [offlineRuntimeSource, setOfflineRuntimeSource] = useState(DEFAULT_OFFLINE_RUNTIME_URL);
  const [offlineRuntimeSha256, setOfflineRuntimeSha256] = useState("");
  const [offlineRuntimeBusy, setOfflineRuntimeBusy] = useState(false);
  const [offlineRuntimeMessage, setOfflineRuntimeMessage] = useState("");
  const [offlineRuntimeError, setOfflineRuntimeError] = useState("");
  const [cloudflareQuotaState, setCloudflareQuotaState] =
    useState<CloudflareQuotaState>(() =>
      getCloudflareQuotaState(typeof window === "undefined" ? undefined : window.localStorage),
    );
  const [saved, setSaved] = useState(false);
  const [validationBusy, setValidationBusy] = useState(false);
  const [validationResults, setValidationResults] = useState<ApiValidationResult[]>([]);
  const [validationError, setValidationError] = useState("");
  const [diagnosticsMessage, setDiagnosticsMessage] = useState("");
  const [diagnosticsError, setDiagnosticsError] = useState("");
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    loadKeys();
  }, []);

  const loadKeys = async () => {
    try {
      const keys = await getApiKeys();
      setGroq(keys.groq || "");
      setGemini(keys.gemini || "");
      setCloudflareAccountId(keys.cloudflareAccountId || "");
      setCloudflareApiToken(keys.cloudflareApiToken || "");
      setDeepgramApiKey(keys.deepgramApiKey || "");
      setTranscriptionProfile(keys.transcriptionProfile || "smart-low-cost");
      setManualTranscriptionProvider(keys.manualTranscriptionProvider || "groq");
      setExpectedSpeakers(keys.expectedSpeakers ? String(keys.expectedSpeakers) : "");
    } catch {
      // First run, no keys yet
    } finally {
      setCloudflareQuotaState(
        getCloudflareQuotaState(typeof window === "undefined" ? undefined : window.localStorage),
      );
      checkLocalTranscriptionBackends()
        .then(setLocalStatus)
        .catch(() =>
          setLocalStatus({
            fasterWhisperAvailable: false,
            parakeetAvailable: false,
          }),
        );
      getOfflineTranscriptionRuntimeStatus()
        .then(setOfflineRuntimeStatus)
        .catch(() => setOfflineRuntimeStatus(null));
      setLoading(false);
    }
  };

  const refreshLocalRuntime = async () => {
    const [local, offline] = await Promise.all([
      checkLocalTranscriptionBackends().catch(() => ({
        fasterWhisperAvailable: false,
        parakeetAvailable: false,
      })),
      getOfflineTranscriptionRuntimeStatus().catch(() => null),
    ]);
    setLocalStatus(local);
    setOfflineRuntimeStatus(offline);
  };

  const installOfflineRuntime = async (source: string) => {
    const normalizedSource = source.trim();
    if (!normalizedSource) {
      setOfflineRuntimeError("Informe uma URL ou selecione um pacote ZIP.");
      return;
    }
    setOfflineRuntimeBusy(true);
    setOfflineRuntimeError("");
    setOfflineRuntimeMessage("Instalando pacote offline. Isso pode levar alguns minutos.");
    try {
      const status = await installOfflineTranscriptionRuntime(
        normalizedSource,
        offlineRuntimeSha256.trim() || undefined,
      );
      setOfflineRuntimeStatus(status);
      await refreshLocalRuntime();
      setOfflineRuntimeMessage("Modo offline instalado e pronto para fallback local.");
    } catch (error) {
      setOfflineRuntimeError(error instanceof Error ? error.message : String(error));
      setOfflineRuntimeMessage("");
    } finally {
      setOfflineRuntimeBusy(false);
    }
  };

  const installOfflineRuntimeFromUrl = () => installOfflineRuntime(offlineRuntimeSource);

  const installOfflineRuntimeFromFile = async () => {
    const selected = await open({
      multiple: false,
      filters: [{ name: "Pacote offline", extensions: ["zip"] }],
    });
    if (typeof selected === "string") {
      setOfflineRuntimeSource(selected);
      await installOfflineRuntime(selected);
    }
  };

  const removeOfflineRuntime = async () => {
    setOfflineRuntimeBusy(true);
    setOfflineRuntimeError("");
    setOfflineRuntimeMessage("Removendo runtime offline.");
    try {
      const status = await removeOfflineTranscriptionRuntime();
      setOfflineRuntimeStatus(status);
      await refreshLocalRuntime();
      setOfflineRuntimeMessage("Runtime offline removido.");
    } catch (error) {
      setOfflineRuntimeError(error instanceof Error ? error.message : String(error));
      setOfflineRuntimeMessage("");
    } finally {
      setOfflineRuntimeBusy(false);
    }
  };

  const clearCloudflareStatus = () => {
    clearCloudflareQuotaExhausted(typeof window === "undefined" ? undefined : window.localStorage);
    setCloudflareQuotaState(
      getCloudflareQuotaState(typeof window === "undefined" ? undefined : window.localStorage),
    );
  };

  const cloudflareConfigured = Boolean(cloudflareAccountId.trim() && cloudflareApiToken.trim());
  const deepgramConfigured = Boolean(deepgramApiKey.trim());
  const groqConfigured = Boolean(groq.trim());
  const geminiConfigured = Boolean(gemini.trim());
  const localConfigured = Boolean(
    localStatus?.fasterWhisperAvailable || localStatus?.parakeetAvailable,
  );
  const offlineRuntimeReady = Boolean(offlineRuntimeStatus?.fasterWhisperAvailable);
  const validationByProvider = new Map(validationResults.map((item) => [item.provider, item]));
  const validationTone = (provider: ApiValidationResult["provider"], configured: boolean) => {
    const result = validationByProvider.get(provider);
    if (!result) return configured ? "good" : "muted";
    if (result.status === "valid") return "good";
    if (result.status === "missing") return "muted";
    return "warn";
  };
  const validationStatus = (
    provider: ApiValidationResult["provider"],
    fallback: string,
  ) => {
    const result = validationByProvider.get(provider);
    if (!result) return fallback;
    const prefix =
      result.status === "valid"
        ? "Validado"
        : result.status === "invalid"
          ? "Invalido"
          : result.status === "missing"
            ? "Nao configurado"
            : "Indisponivel";
    return `${prefix}: ${result.message}`;
  };

  const handleSave = async () => {
    const speakerCount = expectedSpeakers ? Number(expectedSpeakers) : undefined;
    await setApiKeys(
      groq,
      gemini,
      cloudflareAccountId,
      cloudflareApiToken,
      deepgramApiKey,
      transcriptionProfile,
      manualTranscriptionProvider,
      undefined,
      speakerCount,
    );
    setSaved(true);
    setTimeout(() => setSaved(false), 3000);
  };

  const handleValidateKeys = async () => {
    setValidationBusy(true);
    setValidationError("");
    try {
      const results = await validateApiKeys({
        groq,
        gemini,
        cloudflareAccountId,
        cloudflareApiToken,
        deepgramApiKey,
      });
      setValidationResults(results);
    } catch (error) {
      setValidationError(error instanceof Error ? error.message : String(error));
    } finally {
      setValidationBusy(false);
    }
  };

  const handleSaveAndValidate = async () => {
    await handleSave();
    await handleValidateKeys();
  };

  const handleExportDiagnostics = async () => {
    setDiagnosticsMessage("");
    setDiagnosticsError("");
    try {
      const path = await exportDiagnostics(null);
      setDiagnosticsMessage(`Diagnostico exportado: ${path}`);
    } catch (error) {
      setDiagnosticsError(error instanceof Error ? error.message : String(error));
    }
  };

  if (loading) {
    return (
      <div className="mx-auto max-w-3xl">
        <div className="rounded-xl border border-gray-200 bg-white p-6 text-sm text-gray-500 shadow-sm">
          Carregando configuracoes...
        </div>
      </div>
    );
  }

  return (
    <div className="mx-auto max-w-3xl space-y-6">
      <header>
        <h2 className="text-2xl font-bold text-gray-950">Configuracoes</h2>
        <p className="mt-2 text-sm leading-6 text-gray-600">
          Guarde as chaves usadas nas etapas que ainda dependem de API. Os segredos ficam no
          cofre nativo do Windows; apenas perfis e preferencias ficam no arquivo de configuracao.
        </p>
      </header>

      <div className="rounded-xl border border-gray-200 bg-white p-5 shadow-sm md:p-6">
        <div className="mb-5 rounded-lg border border-blue-100 bg-blue-50 px-4 py-3">
          <p className="text-sm font-semibold text-blue-900">Transcricao adaptativa</p>
          <p className="mt-1 text-xs leading-5 text-blue-700">
            Cloudflare e o padrao economico, Deepgram entra para qualidade maxima, Groq continua
            disponivel no modo turbo e Parakeet fica como rota offline.
          </p>
        </div>

        <section className="mb-5 rounded-lg border border-gray-200 bg-gray-50 px-4 py-3">
          <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
            <div>
              <p className="text-sm font-semibold text-gray-950">Diagnostico de provedores</p>
              <p className="mt-1 text-xs leading-5 text-gray-500">
                O processamento usa esse estado para escolher fallback antes de iniciar.
              </p>
            </div>
            {cloudflareQuotaState.isExhaustedToday && (
              <button
                type="button"
                onClick={clearCloudflareStatus}
                className="rounded-lg border border-amber-200 bg-white px-3 py-2 text-xs font-semibold text-amber-700 hover:bg-amber-50"
              >
                Reativar Cloudflare
              </button>
            )}
          </div>
          <div className="mt-4 grid gap-3 sm:grid-cols-2">
            {[
              {
                name: "Cloudflare",
                status: cloudflareConfigured
                  ? cloudflareQuotaState.isExhaustedToday
                    ? "Cota provavelmente esgotada hoje"
                    : validationStatus("cloudflare", "Configurado")
                  : "Nao configurado",
                tone: cloudflareQuotaState.isExhaustedToday
                  ? "warn"
                  : validationTone("cloudflare", cloudflareConfigured),
              },
              {
                name: "Deepgram",
                status: deepgramConfigured
                  ? validationStatus("deepgram", "Configurado para fallback/premium")
                  : "Nao configurado",
                tone: validationTone("deepgram", deepgramConfigured),
              },
              {
                name: "Groq",
                status: groqConfigured
                  ? validationStatus("groq", "Configurado como opcional")
                  : "Nao configurado",
                tone: validationTone("groq", groqConfigured),
              },
              {
                name: "Gemini",
                status: geminiConfigured
                  ? validationStatus("gemini", "Configurado para gerar ata")
                  : "Nao configurado",
                tone: validationTone("gemini", geminiConfigured),
              },
              {
                name: "Local",
                status: localConfigured
                  ? `Disponivel (${[
                      localStatus?.fasterWhisperAvailable ? "faster-whisper" : "",
                      localStatus?.parakeetAvailable ? "Parakeet" : "",
                    ]
                      .filter(Boolean)
                      .join(", ")})`
                  : offlineRuntimeStatus?.installed
                    ? "Runtime offline instalado, mas backend invalido"
                    : "Runtime offline nao instalado",
                tone: localConfigured ? "good" : "warn",
              },
            ].map((item) => (
              <div key={item.name} className="rounded-lg border border-gray-200 bg-white px-3 py-2">
                <p className="text-xs font-medium uppercase tracking-wide text-gray-500">
                  {item.name}
                </p>
                <p
                  className={`mt-1 text-sm font-semibold ${
                    item.tone === "good"
                      ? "text-emerald-700"
                      : item.tone === "warn"
                        ? "text-amber-700"
                        : "text-gray-500"
                  }`}
                >
                  {item.status}
                </p>
              </div>
            ))}
          </div>
        </section>

        <section className="mb-5 rounded-lg border border-gray-200 bg-white px-4 py-4">
          <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
            <div>
              <p className="text-sm font-semibold text-gray-950">Suporte e diagnostico</p>
              <p className="mt-1 text-xs leading-5 text-gray-500">
                Exporte um pacote com metadados e arquivos textuais mascarados. Chaves e tokens sao
                removidos antes de salvar o ZIP.
              </p>
            </div>
            <button
              type="button"
              onClick={handleExportDiagnostics}
              className="rounded-lg border border-gray-300 bg-white px-4 py-2.5 text-sm font-semibold text-gray-800 shadow-sm transition-colors hover:bg-gray-50"
            >
              Exportar diagnostico
            </button>
          </div>
          {diagnosticsMessage && (
            <p className="mt-3 break-all text-sm font-medium text-blue-700">{diagnosticsMessage}</p>
          )}
          {diagnosticsError && (
            <p className="mt-3 rounded-lg border border-red-100 bg-red-50 px-3 py-2 text-sm font-medium text-red-700">
              {diagnosticsError}
            </p>
          )}
        </section>

        <section className="mb-5 rounded-lg border border-gray-200 bg-white px-4 py-4">
          <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
            <div>
              <p className="text-sm font-semibold text-gray-950">Modo offline para usuarios comuns</p>
              <p className="mt-1 text-xs leading-5 text-gray-500">
                Instala um pacote local de transcricao em AppData. Depois disso, o app pode cair
                para faster-whisper sem terminal, Python manual ou chaves de API.
              </p>
            </div>
            <span
              className={`inline-flex w-fit rounded-full px-2.5 py-1 text-xs font-semibold ${
                offlineRuntimeReady
                  ? "bg-emerald-50 text-emerald-700"
                  : "bg-amber-50 text-amber-700"
              }`}
            >
              {offlineRuntimeReady ? "Pronto" : "Nao instalado"}
            </span>
          </div>

          <div className="mt-4 grid gap-3 md:grid-cols-3">
            <div className="rounded-lg border border-gray-200 bg-gray-50 px-3 py-2">
              <p className="text-xs font-medium uppercase tracking-wide text-gray-500">Backend</p>
              <p className="mt-1 text-sm font-semibold text-gray-900">
                {offlineRuntimeStatus?.fasterWhisperAvailable ? "faster-whisper" : "Indisponivel"}
              </p>
            </div>
            <div className="rounded-lg border border-gray-200 bg-gray-50 px-3 py-2">
              <p className="text-xs font-medium uppercase tracking-wide text-gray-500">Versao</p>
              <p className="mt-1 text-sm font-semibold text-gray-900">
                {offlineRuntimeStatus?.version || "Nao informada"}
              </p>
            </div>
            <div className="rounded-lg border border-gray-200 bg-gray-50 px-3 py-2">
              <p className="text-xs font-medium uppercase tracking-wide text-gray-500">Tamanho</p>
              <p className="mt-1 text-sm font-semibold text-gray-900">
                {formatBytes(offlineRuntimeStatus?.sizeBytes || 0)}
              </p>
            </div>
          </div>

          <p className="mt-3 break-all rounded-lg bg-gray-50 px-3 py-2 text-xs text-gray-500">
            {offlineRuntimeStatus?.rootPath || "Caminho do runtime ainda nao resolvido."}
          </p>

          <div className="mt-4 space-y-3">
            <div>
              <label htmlFor="offline-runtime-source" className="mb-1.5 block text-sm font-semibold text-gray-800">
                URL ou caminho do pacote ZIP offline
              </label>
              <input
                id="offline-runtime-source"
                value={offlineRuntimeSource}
                onChange={(event) => setOfflineRuntimeSource(event.target.value)}
                className="w-full rounded-lg border border-gray-300 px-3 py-2.5 text-sm shadow-sm focus:border-blue-500 focus:ring-2 focus:ring-blue-100"
                placeholder={DEFAULT_OFFLINE_RUNTIME_URL}
              />
            </div>
            <div>
              <label htmlFor="offline-runtime-sha" className="mb-1.5 block text-sm font-semibold text-gray-800">
                SHA-256 esperado (opcional)
              </label>
              <input
                id="offline-runtime-sha"
                value={offlineRuntimeSha256}
                onChange={(event) => setOfflineRuntimeSha256(event.target.value)}
                className="w-full rounded-lg border border-gray-300 px-3 py-2.5 text-sm shadow-sm focus:border-blue-500 focus:ring-2 focus:ring-blue-100"
                placeholder="Cole o hash para validar o pacote antes de instalar"
              />
            </div>
          </div>

          <div className="mt-4 flex flex-col gap-2 sm:flex-row sm:items-center">
            <button
              type="button"
              onClick={installOfflineRuntimeFromUrl}
              disabled={offlineRuntimeBusy}
              className="rounded-lg bg-gray-950 px-4 py-2.5 text-sm font-semibold text-white shadow-sm transition-colors hover:bg-gray-800 disabled:cursor-not-allowed disabled:bg-gray-400"
            >
              Instalar/atualizar offline
            </button>
            <button
              type="button"
              onClick={installOfflineRuntimeFromFile}
              disabled={offlineRuntimeBusy}
              className="rounded-lg border border-gray-300 bg-white px-4 py-2.5 text-sm font-semibold text-gray-800 shadow-sm transition-colors hover:bg-gray-50 disabled:cursor-not-allowed disabled:text-gray-400"
            >
              Selecionar ZIP
            </button>
            {offlineRuntimeStatus?.installed && (
              <button
                type="button"
                onClick={removeOfflineRuntime}
                disabled={offlineRuntimeBusy}
                className="rounded-lg border border-red-200 bg-white px-4 py-2.5 text-sm font-semibold text-red-700 shadow-sm transition-colors hover:bg-red-50 disabled:cursor-not-allowed disabled:text-red-300"
              >
                Remover offline
              </button>
            )}
          </div>
          {offlineRuntimeMessage && (
            <p className="mt-3 text-sm font-medium text-blue-700">{offlineRuntimeMessage}</p>
          )}
          {offlineRuntimeError && (
            <p className="mt-3 rounded-lg border border-red-100 bg-red-50 px-3 py-2 text-sm font-medium text-red-700">
              {offlineRuntimeError}
            </p>
          )}
        </section>

        <div className="space-y-5">
        <div>
          <label htmlFor="transcription-profile" className="mb-1.5 block text-sm font-semibold text-gray-800">
            Orcamento padrao de transcricao
          </label>
          <select
            id="transcription-profile"
            value={transcriptionProfile}
            onChange={(e) => setTranscriptionProfile(e.target.value as TranscriptionRoutingProfile)}
            className="w-full rounded-lg border border-gray-300 px-3 py-2.5 text-sm shadow-sm focus:border-blue-500 focus:ring-2 focus:ring-blue-100"
          >
            <option value="smart-low-cost">Baixo custo: Cloudflare + fallback</option>
            <option value="max-quality">Qualidade maxima: Deepgram direto</option>
            <option value="offline-free">R$ 0: offline/local</option>
            <option value="groq-turbo">Groq turbo</option>
            <option value="manual">Manual</option>
          </select>
        </div>
        {transcriptionProfile === "manual" && (
          <div>
            <label htmlFor="manual-transcription-provider" className="mb-1.5 block text-sm font-semibold text-gray-800">
              Provedor manual
            </label>
            <select
              id="manual-transcription-provider"
              value={manualTranscriptionProvider}
              onChange={(e) =>
                setManualTranscriptionProvider(e.target.value as TranscriptionBackend)
              }
              className="w-full rounded-lg border border-gray-300 px-3 py-2.5 text-sm shadow-sm focus:border-blue-500 focus:ring-2 focus:ring-blue-100"
            >
              <option value="cloudflare">Cloudflare Whisper</option>
              <option value="deepgram">Deepgram Nova-3</option>
              <option value="groq">Groq Whisper</option>
              <option value="parakeet-local">Parakeet local</option>
              <option value="local">faster-whisper local</option>
            </select>
          </div>
        )}
        <div>
          <label htmlFor="groq-api-key" className="mb-1.5 block text-sm font-semibold text-gray-800">
            Chave API Groq (opcional para reunioes curtas)
          </label>
          <input
            id="groq-api-key"
            type="password"
            value={groq}
            onChange={(e) => setGroq(e.target.value)}
            placeholder="gsk_..."
            className="w-full rounded-lg border border-gray-300 px-3 py-2.5 text-sm shadow-sm focus:border-blue-500 focus:ring-2 focus:ring-blue-100"
          />
        </div>
        <div className="grid gap-4 md:grid-cols-2">
          <div>
            <label htmlFor="cloudflare-account-id" className="mb-1.5 block text-sm font-semibold text-gray-800">
              Cloudflare Account ID
            </label>
            <input
              id="cloudflare-account-id"
              type="password"
              value={cloudflareAccountId}
              onChange={(e) => setCloudflareAccountId(e.target.value)}
              placeholder="account id"
              className="w-full rounded-lg border border-gray-300 px-3 py-2.5 text-sm shadow-sm focus:border-blue-500 focus:ring-2 focus:ring-blue-100"
            />
          </div>
          <div>
            <label htmlFor="cloudflare-api-token" className="mb-1.5 block text-sm font-semibold text-gray-800">
              Cloudflare API Token
            </label>
            <input
              id="cloudflare-api-token"
              type="password"
              value={cloudflareApiToken}
              onChange={(e) => setCloudflareApiToken(e.target.value)}
              placeholder="cfat_..."
              className="w-full rounded-lg border border-gray-300 px-3 py-2.5 text-sm shadow-sm focus:border-blue-500 focus:ring-2 focus:ring-blue-100"
            />
          </div>
        </div>
        <div>
          <label htmlFor="deepgram-api-key" className="mb-1.5 block text-sm font-semibold text-gray-800">
            Chave API Deepgram
          </label>
          <input
            id="deepgram-api-key"
            type="password"
            value={deepgramApiKey}
            onChange={(e) => setDeepgramApiKey(e.target.value)}
            placeholder="deepgram key"
            className="w-full rounded-lg border border-gray-300 px-3 py-2.5 text-sm shadow-sm focus:border-blue-500 focus:ring-2 focus:ring-blue-100"
          />
        </div>
        <div>
          <label htmlFor="gemini-api-key" className="mb-1.5 block text-sm font-semibold text-gray-800">
            Chave API Gemini
          </label>
          <input
            id="gemini-api-key"
            type="password"
            value={gemini}
            onChange={(e) => setGemini(e.target.value)}
            placeholder="AIza..."
            className="w-full rounded-lg border border-gray-300 px-3 py-2.5 text-sm shadow-sm focus:border-blue-500 focus:ring-2 focus:ring-blue-100"
          />
        </div>
        <div>
          <label htmlFor="expected-speakers" className="mb-1.5 block text-sm font-semibold text-gray-800">
            Numero esperado de falantes
          </label>
          <select
            id="expected-speakers"
            value={expectedSpeakers}
            onChange={(e) => setExpectedSpeakers(e.target.value)}
            className="w-full rounded-lg border border-gray-300 px-3 py-2.5 text-sm shadow-sm focus:border-blue-500 focus:ring-2 focus:ring-blue-100"
          >
            <option value="">Nao sei, detectar automaticamente</option>
            {[2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12].map((count) => (
              <option key={count} value={count}>
                {count} falantes
              </option>
            ))}
          </select>
          <p className="mt-1.5 text-xs leading-5 text-gray-500">
            Quando voce souber esse numero, a separacao de falantes fica mais estavel.
          </p>
        </div>
          <div className="flex flex-col gap-3 border-t border-gray-100 pt-5 sm:flex-row sm:items-center">
            <button
              onClick={handleSave}
              className="rounded-lg bg-blue-600 px-5 py-2.5 text-sm font-semibold text-white shadow-sm transition-colors hover:bg-blue-700"
            >
              Salvar configuracoes
            </button>
            <button
              onClick={handleSaveAndValidate}
              disabled={validationBusy}
              className="rounded-lg border border-blue-200 bg-blue-50 px-5 py-2.5 text-sm font-semibold text-blue-700 shadow-sm transition-colors hover:bg-blue-100 disabled:cursor-not-allowed disabled:opacity-60"
            >
              {validationBusy ? "Testando..." : "Salvar e testar chaves"}
            </button>
            {saved && (
              <p className="text-sm font-medium text-green-700">Chaves salvas com sucesso.</p>
            )}
            {validationError && (
              <p className="text-sm font-medium text-red-700">{validationError}</p>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
