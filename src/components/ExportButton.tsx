import { useState } from "react";
import minutesCss from "../templates/minutes.css?raw";
import { buildStandaloneMinutesHtml } from "../lib/executiveMinutes";
import { saveHtml, savePdf, openFolder } from "../lib/tauri";

interface Props {
  title: string;
  executiveHtml?: string | null;
}

type ExportKind = "executive-pdf" | "complete-pdf" | "complete-html";

function safeFileName(title: string, suffix: string, extension: string) {
  return `${title || "ata"}_${suffix}.${extension}`.replace(/[^a-zA-Z0-9._-]/g, "_");
}

function openSavedFolder(savedPath: string) {
  const lastSeparator = Math.max(savedPath.lastIndexOf("\\"), savedPath.lastIndexOf("/"));
  const folder = lastSeparator >= 0 ? savedPath.substring(0, lastSeparator) : savedPath;
  return openFolder(folder);
}

function createTemporaryMinutesElement(html: string) {
  const element = document.createElement("div");
  element.className = "minutes-wrapper";
  element.innerHTML = html;
  return element;
}

export default function ExportButton({ title, executiveHtml }: Props) {
  const [exporting, setExporting] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);

  const exportPdfFromElement = async (element: HTMLElement, suffix: string) => {
    const { exportToPDF } = await import("../lib/pdfExport");
    const pdfBytes = await exportToPDF(element, `${title} - ${suffix}`);
    const savedPath = await savePdf(Array.from(pdfBytes), safeFileName(title, suffix, "pdf"));
    if (savedPath) await openSavedFolder(savedPath);
  };

  const handleExport = async (kind: ExportKind) => {
    const completeElement = document.getElementById("minutes-preview");
    if (!completeElement) return;

    setMenuOpen(false);
    setExporting(true);
    try {
      if (kind === "executive-pdf") {
        if (executiveHtml) {
          const temporaryElement = createTemporaryMinutesElement(executiveHtml);
          try {
            await exportPdfFromElement(temporaryElement, "executiva");
          } finally {
            temporaryElement.remove();
          }
        } else {
          await exportPdfFromElement(completeElement, "executiva");
        }
        return;
      }

      if (kind === "complete-pdf") {
        await exportPdfFromElement(completeElement, "completa");
        return;
      }

      const standaloneHtml = buildStandaloneMinutesHtml(
        completeElement.innerHTML,
        `${title} - Ata completa`,
        minutesCss,
      );
      const savedPath = await saveHtml(standaloneHtml, safeFileName(title, "completa", "html"));
      if (savedPath) await openSavedFolder(savedPath);
    } finally {
      setExporting(false);
    }
  };

  return (
    <div className="relative inline-flex">
      <button
        type="button"
        onClick={() => handleExport("executive-pdf")}
        disabled={exporting}
        className="rounded-l-lg bg-green-600 px-4 py-2.5 text-sm font-semibold text-white shadow-sm transition-colors hover:bg-green-700 disabled:cursor-not-allowed disabled:opacity-50"
      >
        {exporting ? "Exportando..." : "PDF executivo"}
      </button>
      <button
        type="button"
        aria-label="Abrir opcoes de exportacao"
        aria-expanded={menuOpen}
        onClick={() => setMenuOpen((open) => !open)}
        disabled={exporting}
        className="rounded-r-lg border-l border-green-500 bg-green-600 px-3 py-2.5 text-sm font-semibold text-white shadow-sm transition-colors hover:bg-green-700 disabled:cursor-not-allowed disabled:opacity-50"
      >
        v
      </button>

      {menuOpen && (
        <div className="export-menu absolute right-0 top-full z-20 mt-2 w-48 overflow-hidden rounded-lg border border-gray-200 bg-white py-1 text-sm shadow-lg">
          <button
            type="button"
            onClick={() => handleExport("executive-pdf")}
            className="export-menu-item block w-full px-3 py-2 text-left font-medium text-gray-800 hover:bg-gray-50"
          >
            PDF executivo
          </button>
          <button
            type="button"
            onClick={() => handleExport("complete-pdf")}
            className="export-menu-item block w-full px-3 py-2 text-left font-medium text-gray-800 hover:bg-gray-50"
          >
            PDF completo
          </button>
          <button
            type="button"
            onClick={() => handleExport("complete-html")}
            className="export-menu-item block w-full px-3 py-2 text-left font-medium text-gray-800 hover:bg-gray-50"
          >
            HTML completo
          </button>
        </div>
      )}
    </div>
  );
}
