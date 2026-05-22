import { useEffect, useState } from "react";
import { getApiKeys, setApiKeys } from "../lib/tauri";
import type { TranscriptionBackend } from "../lib/transcriptionProvider";
import type { TranscriptionRoutingProfile } from "../lib/types";

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
  const [saved, setSaved] = useState(false);
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
      setLoading(false);
    }
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
      speakerCount,
    );
    setSaved(true);
    setTimeout(() => setSaved(false), 3000);
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
          Guarde as chaves usadas nas etapas que ainda dependem de API. Elas ficam no
          armazenamento local do app.
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

        <div className="space-y-5">
        <div>
          <label htmlFor="transcription-profile" className="mb-1.5 block text-sm font-semibold text-gray-800">
            Perfil de transcricao
          </label>
          <select
            id="transcription-profile"
            value={transcriptionProfile}
            onChange={(e) => setTranscriptionProfile(e.target.value as TranscriptionRoutingProfile)}
            className="w-full rounded-lg border border-gray-300 px-3 py-2.5 text-sm shadow-sm focus:border-blue-500 focus:ring-2 focus:ring-blue-100"
          >
            <option value="smart-low-cost">Economico inteligente</option>
            <option value="max-quality">Qualidade maxima</option>
            <option value="groq-turbo">Groq turbo</option>
            <option value="offline-free">Offline gratis</option>
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
            {[2, 3, 4, 5, 6, 7, 8].map((count) => (
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
            {saved && (
              <p className="text-sm font-medium text-green-700">Chaves salvas com sucesso.</p>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
