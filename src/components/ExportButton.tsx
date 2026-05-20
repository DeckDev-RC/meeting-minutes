import { useState } from "react";
import { savePdf, openFolder } from "../lib/tauri";

interface Props {
  title: string;
}

export default function ExportButton({ title }: Props) {
  const [exporting, setExporting] = useState(false);

  const handleExport = async () => {
    const el = document.getElementById("minutes-preview");
    if (!el) return;

    setExporting(true);
    try {
      const { exportToPDF } = await import("../lib/pdfExport");
      const pdfBytes = await exportToPDF(el, title);
      const fileName = `${title || "ata"}.pdf`.replace(/[^a-zA-Z0-9._-]/g, "_");
      const savedPath = await savePdf(Array.from(pdfBytes), fileName);
      if (savedPath) {
        const lastSeparator = Math.max(savedPath.lastIndexOf("\\"), savedPath.lastIndexOf("/"));
        const folder = lastSeparator >= 0 ? savedPath.substring(0, lastSeparator) : savedPath;
        await openFolder(folder);
      }
    } finally {
      setExporting(false);
    }
  };

  return (
    <button
      onClick={handleExport}
      disabled={exporting}
      className="rounded-lg bg-green-600 px-4 py-2.5 text-sm font-semibold text-white shadow-sm transition-colors hover:bg-green-700 disabled:cursor-not-allowed disabled:opacity-50"
    >
      {exporting ? "Exportando..." : "Exportar PDF"}
    </button>
  );
}
