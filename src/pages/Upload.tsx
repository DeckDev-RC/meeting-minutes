import { useState } from "react";
import { useNavigate } from "react-router-dom";
import DropZone from "../components/DropZone";
import { saveMeeting } from "../lib/tauri";
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

export default function Upload() {
  const navigate = useNavigate();
  const [filePath, setFilePath] = useState<string | null>(null);
  const [processingProfile, setProcessingProfile] = useState<ProcessingProfile>("balanced");
  const [participantsHint, setParticipantsHint] = useState("");
  const [loading, setLoading] = useState(false);
  const { setCurrentMeeting, reset } = useMeetingStore();

  const handleProcess = async () => {
    if (!filePath) return;
    setLoading(true);
    try {
      reset();
      const id = await saveMeeting({
        filePath,
        participantsHint: participantsHint.trim() || null,
        processingProfile,
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
            <div className="mt-6 flex flex-col gap-3 border-t border-gray-100 pt-5 sm:flex-row sm:items-center sm:justify-between">
              <p className="text-sm text-gray-500">Tudo certo para iniciar o processamento.</p>
              <button
                onClick={handleProcess}
                disabled={loading}
                className="rounded-lg bg-blue-600 px-6 py-3 text-sm font-semibold text-white shadow-sm transition-colors hover:bg-blue-700 disabled:cursor-not-allowed disabled:opacity-50"
              >
                {loading ? "Iniciando..." : "Processar reuniao"}
              </button>
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
