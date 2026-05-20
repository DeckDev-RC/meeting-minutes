import { useState, useCallback } from "react";
import { open } from "@tauri-apps/plugin-dialog";

interface DropZoneProps {
  onFileSelected: (path: string) => void;
}

const ACCEPTED_EXTENSIONS = [".mp4", ".mp3", ".wav", ".m4a", ".webm"];

export default function DropZone({ onFileSelected }: DropZoneProps) {
  const [dragging, setDragging] = useState(false);
  const [selectedFile, setSelectedFile] = useState<string | null>(null);

  const handleDrop = useCallback(
    (e: React.DragEvent) => {
      e.preventDefault();
      setDragging(false);
      const files = e.dataTransfer.files;
      if (files.length > 0) {
        const path = (files[0] as any).path || files[0].name;
        const ext = path.slice(path.lastIndexOf(".")).toLowerCase();
        if (ACCEPTED_EXTENSIONS.includes(ext)) {
          setSelectedFile(path);
          onFileSelected(path);
        }
      }
    },
    [onFileSelected]
  );

  const handleBrowse = async () => {
    const selected = await open({
      multiple: false,
      filters: [
        {
          name: "Audio/Video",
          extensions: ["mp4", "mp3", "wav", "m4a", "webm"],
        },
      ],
    });
    if (selected) {
      setSelectedFile(selected as string);
      onFileSelected(selected as string);
    }
  };

  return (
    <div
      onDragOver={(e) => {
        e.preventDefault();
        setDragging(true);
      }}
      onDragLeave={() => setDragging(false)}
      onDrop={handleDrop}
      className={`rounded-xl border border-dashed p-8 text-center transition-colors md:p-12 ${
        dragging
          ? "border-blue-400 bg-blue-50"
          : "border-gray-300 bg-white hover:border-blue-300 hover:bg-blue-50/30"
      }`}
    >
      {selectedFile ? (
        <div className="mx-auto max-w-xl">
          <div className="mx-auto mb-4 flex h-12 w-12 items-center justify-center rounded-xl bg-green-50 text-sm font-bold text-green-700">
            OK
          </div>
          <p className="text-sm font-semibold text-gray-900">Arquivo pronto para processar</p>
          <p className="mt-2 rounded-lg bg-gray-50 px-3 py-2 text-sm font-medium text-gray-700 break-all">
            {selectedFile}
          </p>
        </div>
      ) : (
        <div className="mx-auto max-w-md">
          <div className="mx-auto mb-5 flex h-14 w-14 items-center justify-center rounded-2xl bg-blue-50 text-sm font-bold text-blue-700">
            UP
          </div>
          <p className="text-base font-semibold text-gray-900">Arraste seu audio ou video</p>
          <p className="mt-2 text-sm leading-6 text-gray-500">
            Use arquivos MP4, MP3, WAV, M4A ou WEBM. Videos longos tambem entram no fluxo
            otimizado de blocos.
          </p>
          <button
            onClick={handleBrowse}
            className="mt-5 rounded-lg bg-blue-600 px-5 py-2.5 text-sm font-semibold text-white shadow-sm transition-colors hover:bg-blue-700"
          >
            Selecionar arquivo
          </button>
        </div>
      )}
    </div>
  );
}
