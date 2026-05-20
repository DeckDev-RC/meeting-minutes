import { useEffect } from "react";
import { useNavigate } from "react-router-dom";
import { useMeetingStore } from "../store/meetingStore";
import { getMeetings } from "../lib/tauri";
import { invoke } from "@tauri-apps/api/core";

const statusLabel = {
  pending: "Pendente",
  processing: "Processando",
  done: "Concluida",
  error: "Erro",
} as const;

export default function History() {
  const navigate = useNavigate();
  const { meetings, setMeetings } = useMeetingStore();

  useEffect(() => {
    loadMeetings();
  }, []);

  const loadMeetings = async () => {
    const data = await getMeetings();
    setMeetings(data);
  };

  const handleDelete = async (id: string) => {
    if (!confirm("Deseja excluir esta reuniao?")) return;
    await invoke("delete_meeting", { id });
    await loadMeetings();
  };

  return (
    <div className="mx-auto max-w-5xl space-y-6">
      <header className="flex flex-col gap-3 sm:flex-row sm:items-end sm:justify-between">
        <div>
          <h2 className="text-2xl font-bold text-gray-950">Historico</h2>
          <p className="mt-2 text-sm leading-6 text-gray-600">
            Acompanhe reunioes em andamento, retome falhas e abra atas ja finalizadas.
          </p>
        </div>
        <button
          onClick={loadMeetings}
          className="rounded-lg border border-gray-200 bg-white px-4 py-2 text-sm font-semibold text-gray-700 shadow-sm hover:bg-gray-50"
        >
          Atualizar
        </button>
      </header>
      {meetings.length === 0 ? (
        <div className="rounded-xl border border-dashed border-gray-300 bg-white p-10 text-center shadow-sm">
          <p className="text-sm font-semibold text-gray-900">Nenhuma reuniao ainda</p>
          <p className="mt-2 text-sm text-gray-500">
            Quando voce processar um arquivo, ele aparecera aqui.
          </p>
        </div>
      ) : (
        <div className="space-y-3">
          {meetings.map((m) => (
            <div
              key={m.id}
              className="flex flex-col gap-4 rounded-xl border border-gray-200 bg-white p-4 shadow-sm sm:flex-row sm:items-center sm:justify-between"
            >
              <div className="min-w-0">
                <p className="truncate font-semibold text-gray-950">
                  {m.title || "Reuniao sem titulo"}
                </p>
                <p className="mt-1 text-xs text-gray-500">
                  {new Date(m.createdAt).toLocaleDateString("pt-BR")} -{" "}
                  <span
                    className={`font-medium ${
                      m.status === "done"
                        ? "text-green-600"
                        : m.status === "error"
                        ? "text-red-600"
                        : "text-yellow-600"
                    }`}
                  >
                    {statusLabel[m.status]}
                  </span>
                </p>
              </div>
              <div className="flex flex-wrap gap-2">
                {m.status === "done" && (
                  <button
                    onClick={() => navigate(`/minutes/${m.id}`)}
                    className="rounded-lg bg-blue-600 px-3 py-2 text-sm font-semibold text-white hover:bg-blue-700"
                  >
                    Ver Ata
                  </button>
                )}
                {m.status === "processing" && (
                  <button
                    onClick={() => navigate(`/processing/${m.id}`)}
                    className="rounded-lg bg-blue-600 px-3 py-2 text-sm font-semibold text-white hover:bg-blue-700"
                  >
                    Acompanhar
                  </button>
                )}
                {m.status === "error" && (
                  <button
                    onClick={() => navigate(`/processing/${m.id}`)}
                    className="rounded-lg bg-yellow-50 px-3 py-2 text-sm font-semibold text-yellow-800 hover:bg-yellow-100"
                  >
                    Retomar
                  </button>
                )}
                <button
                  onClick={() => handleDelete(m.id)}
                  disabled={m.status === "processing"}
                  className="rounded-lg bg-red-50 px-3 py-2 text-sm font-semibold text-red-700 hover:bg-red-100 disabled:cursor-not-allowed disabled:opacity-40"
                >
                  Excluir
                </button>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
