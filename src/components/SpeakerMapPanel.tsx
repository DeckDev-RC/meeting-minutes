import { useEffect, useMemo, useState } from "react";
import { normalizeSpeakerMap, type SpeakerMap } from "../lib/speakerMap";

interface Props {
  labels: string[];
  value: SpeakerMap;
  onSave: (speakerMap: SpeakerMap) => Promise<void>;
}

export default function SpeakerMapPanel({ labels, value, onSave }: Props) {
  const [draft, setDraft] = useState<SpeakerMap>(value);
  const [status, setStatus] = useState<"idle" | "saving" | "saved" | "error">("idle");

  useEffect(() => {
    setDraft(value);
    setStatus("idle");
  }, [value]);

  const normalized = useMemo(() => normalizeSpeakerMap(labels, draft), [draft, labels]);
  const changed = JSON.stringify(normalized) !== JSON.stringify(normalizeSpeakerMap(labels, value));

  const updateSpeaker = (speaker: string, name: string) => {
    setDraft((current) => ({ ...current, [speaker]: name }));
    setStatus("idle");
  };

  const save = async () => {
    setStatus("saving");
    try {
      await onSave(normalized);
      setStatus("saved");
    } catch {
      setStatus("error");
    }
  };

  if (labels.length === 0) {
    return (
      <section className="rounded-xl border border-gray-200 bg-white p-5 text-sm text-gray-500 shadow-sm">
        Nenhum falante identificado para mapear nesta reuniao.
      </section>
    );
  }

  return (
    <section className="rounded-xl border border-gray-200 bg-white p-5 shadow-sm md:p-6">
      <div className="flex flex-col gap-3 border-b border-gray-100 pb-4 sm:flex-row sm:items-start sm:justify-between">
        <div>
          <p className="text-xs font-semibold uppercase tracking-wide text-blue-600">
            Falantes
          </p>
          <h3 className="mt-1 text-xl font-bold text-gray-950">Mapeamento de nomes</h3>
          <p className="mt-2 text-sm leading-6 text-gray-600">
            Renomeie os rotulos da diarizacao para revisar a ata sem reprocessar o audio.
          </p>
        </div>
        <button
          type="button"
          disabled={!changed || status === "saving"}
          onClick={save}
          className="rounded-lg bg-gray-950 px-4 py-2 text-sm font-semibold text-white shadow-sm transition hover:bg-gray-800 disabled:cursor-not-allowed disabled:bg-gray-300"
        >
          {status === "saving" ? "Salvando..." : "Salvar nomes"}
        </button>
      </div>

      <div className="mt-4 grid gap-3 sm:grid-cols-2">
        {labels.map((speaker) => (
          <label key={speaker} className="block">
            <span className="mb-1.5 block text-sm font-semibold text-gray-800">{speaker}</span>
            <input
              value={draft[speaker] ?? ""}
              onChange={(event) => updateSpeaker(speaker, event.target.value)}
              placeholder="Nome real"
              className="w-full rounded-lg border border-gray-200 bg-white px-3 py-2 text-sm outline-none transition focus:border-blue-500 focus:ring-2 focus:ring-blue-100"
            />
          </label>
        ))}
      </div>

      {status === "saved" && (
        <p className="mt-3 rounded-lg bg-emerald-50 px-3 py-2 text-sm text-emerald-700">
          Mapeamento salvo. A previa da ata ja usa os nomes revisados.
        </p>
      )}
      {status === "error" && (
        <p className="mt-3 rounded-lg bg-red-50 px-3 py-2 text-sm text-red-700">
          Nao foi possivel salvar o mapeamento de falantes.
        </p>
      )}
    </section>
  );
}
