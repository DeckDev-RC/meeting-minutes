import { useEffect, useState } from "react";
import { getApiKeys, setApiKeys } from "../lib/tauri";

export default function Settings() {
  const [groq, setGroq] = useState("");
  const [gemini, setGemini] = useState("");
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
      setExpectedSpeakers(keys.expectedSpeakers ? String(keys.expectedSpeakers) : "");
    } catch {
      // First run, no keys yet
    } finally {
      setLoading(false);
    }
  };

  const handleSave = async () => {
    const speakerCount = expectedSpeakers ? Number(expectedSpeakers) : undefined;
    await setApiKeys(groq, gemini, speakerCount);
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
          Guarde as chaves usadas para transcricao e geracao da ata. Elas ficam no armazenamento
          local do app.
        </p>
      </header>

      <div className="rounded-xl border border-gray-200 bg-white p-5 shadow-sm md:p-6">
        <div className="mb-5 rounded-lg border border-blue-100 bg-blue-50 px-4 py-3">
          <p className="text-sm font-semibold text-blue-900">Pipeline gratuito primeiro</p>
          <p className="mt-1 text-xs leading-5 text-blue-700">
            O app usa processamento local para audio e suas chaves apenas nas etapas de IA.
          </p>
        </div>

        <div className="space-y-5">
        <div>
          <label className="mb-1.5 block text-sm font-semibold text-gray-800">
            Chave API Groq (Whisper)
          </label>
          <input
            type="password"
            value={groq}
            onChange={(e) => setGroq(e.target.value)}
            placeholder="gsk_..."
            className="w-full rounded-lg border border-gray-300 px-3 py-2.5 text-sm shadow-sm focus:border-blue-500 focus:ring-2 focus:ring-blue-100"
          />
        </div>
        <div>
          <label className="mb-1.5 block text-sm font-semibold text-gray-800">
            Chave API Gemini
          </label>
          <input
            type="password"
            value={gemini}
            onChange={(e) => setGemini(e.target.value)}
            placeholder="AIza..."
            className="w-full rounded-lg border border-gray-300 px-3 py-2.5 text-sm shadow-sm focus:border-blue-500 focus:ring-2 focus:ring-blue-100"
          />
        </div>
        <div>
          <label className="mb-1.5 block text-sm font-semibold text-gray-800">
            Numero esperado de falantes
          </label>
          <select
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
