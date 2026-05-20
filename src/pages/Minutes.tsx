import { useEffect, useState } from "react";
import { useParams } from "react-router-dom";
import { invoke } from "@tauri-apps/api/core";
import MinutesPreview from "../components/MinutesPreview";
import ExportButton from "../components/ExportButton";

interface MinutesData {
  id: string;
  meeting_id: string;
  html_content: string;
  pdf_path: string | null;
  model_used: string;
  created_at: string;
}

export default function Minutes() {
  const { id } = useParams<{ id: string }>();
  const [html, setHtml] = useState<string | null>(null);
  const title = "Ata de Reuniao";

  useEffect(() => {
    if (!id) return;
    loadMinutes(id);
  }, [id]);

  const loadMinutes = async (meetingId: string) => {
    try {
      const data = await invoke<MinutesData | null>("get_minutes_by_meeting", {
        meetingId,
      });
      if (data) {
        setHtml(data.html_content);
      }
    } catch (err) {
      console.error("Failed to load minutes:", err);
    }
  };

  if (!html) {
    return (
      <div className="flex h-64 items-center justify-center">
        <div className="rounded-xl border border-gray-200 bg-white px-6 py-4 text-sm text-gray-500 shadow-sm">
          Carregando ata...
        </div>
      </div>
    );
  }

  return (
    <div className="mx-auto max-w-5xl space-y-6">
      <div className="flex flex-col gap-3 sm:flex-row sm:items-end sm:justify-between">
        <div>
          <h2 className="text-2xl font-bold text-gray-950">Ata da reuniao</h2>
          <p className="mt-2 text-sm leading-6 text-gray-600">
            Revise o conteudo gerado e exporte em PDF quando estiver pronto.
          </p>
        </div>
        <ExportButton title={title} />
      </div>
      <div className="rounded-xl border border-gray-200 bg-white p-4 shadow-sm md:p-8">
        <MinutesPreview html={html} />
      </div>
    </div>
  );
}
