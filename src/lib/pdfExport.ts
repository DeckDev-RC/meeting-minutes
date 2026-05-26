import jsPDF from 'jspdf';
import html2canvas from 'html2canvas';
import {
  choosePdfRenderStrategy,
  planPdfPageSlices,
  type PdfKeepRange,
  type PdfPageSlice,
} from './pdfLayout';

const EXPORT_WIDTH_PX = 794;
const PDF_CANVAS_SCALE = 2;
const PDF_KEEP_SELECTORS = [
  '.header',
  '.section',
  '.summary-box',
  '.decision-list > li',
  '.timeline-list > li',
  '.trace-list > li',
  '.question-list > li',
  '.risk-list > li',
  '.table-actions tr',
  'blockquote',
  'h2',
].join(',');

export interface PdfExportProgress {
  phase: 'preparing' | 'rendering' | 'assembling' | 'complete';
  percent: number;
  detail: string;
}

export interface PdfExportOptions {
  onProgress?: (progress: PdfExportProgress) => void;
}

function createExportSurface(minutesHtmlElement: HTMLElement) {
  const host = document.createElement('div');
  host.className = 'pdf-export-host';
  host.style.position = 'fixed';
  host.style.left = '-10000px';
  host.style.top = '0';
  host.style.width = `${EXPORT_WIDTH_PX}px`;
  host.style.background = '#ffffff';
  host.style.zIndex = '-1';

  const clone = minutesHtmlElement.cloneNode(true) as HTMLElement;
  clone.removeAttribute('id');
  clone.removeAttribute('style');
  clone.classList.add('pdf-export-document');
  host.appendChild(clone);

  const style = document.createElement('style');
  style.dataset.meetingMinutesPdf = 'true';
  style.textContent = `
    .pdf-export-host {
      color: #0f172a;
      font-family: Inter, Arial, sans-serif;
    }

    .pdf-export-host,
    .pdf-export-host * {
      box-sizing: border-box;
    }

    .pdf-export-document {
      width: ${EXPORT_WIDTH_PX}px;
      min-height: 1123px;
      margin: 0;
      padding: 44px 52px 56px;
      border: 0;
      box-shadow: none;
      background: #ffffff;
    }

    .pdf-export-document .header,
    .pdf-export-document .section,
    .pdf-export-document .summary-box,
    .pdf-export-document table,
    .pdf-export-document tr,
    .pdf-export-document li {
      break-inside: avoid;
      page-break-inside: avoid;
    }
  `;

  document.head.appendChild(style);
  document.body.appendChild(host);

  return {
    host,
    cleanup: () => {
      host.remove();
      style.remove();
    },
  };
}

function addPageFooter(pdf: jsPDF, pageNumber: number, pageCount: number, title: string) {
  const pageWidth = 210;
  const pageHeight = 297;
  const margin = 18;

  pdf.setDrawColor(226, 232, 240);
  pdf.setLineWidth(0.2);
  pdf.line(margin, pageHeight - 13, pageWidth - margin, pageHeight - 13);
  pdf.setFont('helvetica', 'normal');
  pdf.setFontSize(8);
  pdf.setTextColor(100, 116, 139);
  pdf.text(title || 'Meeting Minutes AI', margin, pageHeight - 7);
  pdf.text(`Pagina ${pageNumber} de ${pageCount}`, pageWidth - margin, pageHeight - 7, {
    align: 'right',
  });
}

function collectPdfKeepRanges(host: HTMLElement, pageContentHeightPx: number): PdfKeepRange[] {
  const hostRect = host.getBoundingClientRect();
  const maxKeepHeight = Math.max(24, pageContentHeightPx * 0.92);
  const ranges: PdfKeepRange[] = [];

  host.querySelectorAll<HTMLElement>(PDF_KEEP_SELECTORS).forEach((element) => {
    const rect = element.getBoundingClientRect();
    const top = rect.top - hostRect.top;
    const bottom = rect.bottom - hostRect.top;
    const height = bottom - top;

    if (!Number.isFinite(top) || !Number.isFinite(bottom)) return;
    if (height <= 1 || height > maxKeepHeight) return;

    ranges.push({
      top,
      bottom,
      reason:
        element.tagName.toLowerCase() === 'tr'
          ? 'table-row'
          : element.className?.toString() || element.tagName.toLowerCase(),
    });
  });

  return ranges;
}

function addCanvasPage(
  pdf: jsPDF,
  canvas: HTMLCanvasElement,
  pageIndex: number,
  pageCount: number,
  title: string,
  margin: number,
  pageContentHeight: number,
  pageHeight: number,
  contentWidth: number,
  outputHeightMm: number,
) {
  if (pageIndex > 0) pdf.addPage();
  pdf.addImage(
    canvas,
    'PNG',
    margin,
    margin,
    contentWidth,
    outputHeightMm,
    undefined,
    'FAST',
  );
  pdf.setFillColor(255, 255, 255);
  pdf.rect(0, margin + pageContentHeight, 210, pageHeight - margin - pageContentHeight, 'F');
  addPageFooter(pdf, pageIndex + 1, pageCount, title);
}

function sliceCanvasPage(
  fullCanvas: HTMLCanvasElement,
  page: PdfPageSlice,
): HTMLCanvasElement {
  const scale = fullCanvas.width / EXPORT_WIDTH_PX;
  const sourceY = Math.min(
    Math.max(0, Math.floor(page.sourceY * scale)),
    Math.max(0, fullCanvas.height - 1),
  );
  const sourceHeight = Math.max(
    1,
    Math.min(Math.ceil(page.sourceHeight * scale), fullCanvas.height - sourceY),
  );
  const canvas = document.createElement('canvas');
  canvas.width = fullCanvas.width;
  canvas.height = sourceHeight;
  const context = canvas.getContext('2d');
  if (!context) {
    throw new Error('Nao foi possivel preparar pagina do PDF.');
  }

  context.drawImage(
    fullCanvas,
    0,
    sourceY,
    fullCanvas.width,
    sourceHeight,
    0,
    0,
    fullCanvas.width,
    sourceHeight,
  );

  return canvas;
}

export async function exportToPDF(
  minutesHtmlElement: HTMLElement,
  title: string,
  options: PdfExportOptions = {},
): Promise<Uint8Array> {
  const reportProgress = (progress: PdfExportProgress) => options.onProgress?.(progress);
  const pdf = new jsPDF({
    orientation: 'portrait',
    unit: 'mm',
    format: 'a4',
  });
  pdf.setProperties({ title });

  const pageWidth = 210;
  const pageHeight = 297;
  const margin = 18;
  const footerSpace = 14;
  const contentWidth = pageWidth - margin * 2;

  const surface = createExportSurface(minutesHtmlElement);

  try {
    reportProgress({ phase: 'preparing', percent: 5, detail: 'Preparando documento' });
    await document.fonts?.ready;

    const pageContentHeight = pageHeight - margin * 2 - footerSpace;
    const pageContentHeightPx = Math.max(
      1,
      Math.floor((pageContentHeight / contentWidth) * EXPORT_WIDTH_PX),
    );
    const documentHeightPx = Math.max(
      surface.host.scrollHeight,
      Math.ceil(surface.host.getBoundingClientRect().height),
      1,
    );
    const keepRanges = collectPdfKeepRanges(surface.host, pageContentHeightPx);
    const pageSlices = planPdfPageSlices({
      documentHeightPx,
      exportWidthPx: EXPORT_WIDTH_PX,
      contentWidthMm: contentWidth,
      pageContentHeightMm: pageContentHeight,
      keepRanges,
    });

    const renderStrategy = choosePdfRenderStrategy({
      documentHeightPx,
      exportWidthPx: EXPORT_WIDTH_PX,
      scale: PDF_CANVAS_SCALE,
      pageCount: pageSlices.length,
    });

    if (renderStrategy.mode === 'single-canvas') {
      reportProgress({ phase: 'rendering', percent: 20, detail: 'Renderizando documento' });
      const fullCanvas = await html2canvas(surface.host, {
        scale: PDF_CANVAS_SCALE,
        useCORS: true,
        backgroundColor: '#ffffff',
        width: EXPORT_WIDTH_PX,
        height: documentHeightPx,
        windowWidth: EXPORT_WIDTH_PX,
        windowHeight: documentHeightPx,
        scrollX: 0,
        scrollY: 0,
      });

      for (let pageIndex = 0; pageIndex < pageSlices.length; pageIndex += 1) {
        reportProgress({
          phase: 'assembling',
          percent: Math.min(95, 35 + Math.round(((pageIndex + 1) / pageSlices.length) * 55)),
          detail: `Montando pagina ${pageIndex + 1} de ${pageSlices.length}`,
        });
        const page = pageSlices[pageIndex];
        const pageCanvas =
          pageSlices.length === 1 ? fullCanvas : sliceCanvasPage(fullCanvas, page);
        addCanvasPage(
          pdf,
          pageCanvas,
          pageIndex,
          pageSlices.length,
          title,
          margin,
          pageContentHeight,
          pageHeight,
          contentWidth,
          page.outputHeightMm,
        );
      }
    } else {
      for (let pageIndex = 0; pageIndex < pageSlices.length; pageIndex += 1) {
        const page = pageSlices[pageIndex];
        reportProgress({
          phase: 'rendering',
          percent: Math.min(90, 15 + Math.round((pageIndex / pageSlices.length) * 70)),
          detail: `Renderizando pagina ${pageIndex + 1} de ${pageSlices.length}`,
        });
        const canvas = await html2canvas(surface.host, {
          scale: PDF_CANVAS_SCALE,
          useCORS: true,
          backgroundColor: '#ffffff',
          width: EXPORT_WIDTH_PX,
          height: page.sourceHeight,
          windowWidth: EXPORT_WIDTH_PX,
          windowHeight: page.sourceHeight,
          scrollX: 0,
          scrollY: 0,
          y: page.sourceY,
        });

        addCanvasPage(
          pdf,
          canvas,
          pageIndex,
          pageSlices.length,
          title,
          margin,
          pageContentHeight,
          pageHeight,
          contentWidth,
          page.outputHeightMm,
        );
      }
    }

    reportProgress({ phase: 'complete', percent: 100, detail: 'PDF pronto' });
    const arrayBuffer = pdf.output('arraybuffer');
    return new Uint8Array(arrayBuffer);
  } finally {
    surface.cleanup();
  }
}
