import jsPDF from 'jspdf';
import html2canvas from 'html2canvas';
import { planPdfPageSlices } from './pdfLayout';

const EXPORT_WIDTH_PX = 794;

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

export async function exportToPDF(
  minutesHtmlElement: HTMLElement,
  title: string
): Promise<Uint8Array> {
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
    await document.fonts?.ready;

    const pageContentHeight = pageHeight - margin * 2 - footerSpace;
    const documentHeightPx = Math.max(
      surface.host.scrollHeight,
      Math.ceil(surface.host.getBoundingClientRect().height),
      1,
    );
    const pageSlices = planPdfPageSlices({
      documentHeightPx,
      exportWidthPx: EXPORT_WIDTH_PX,
      contentWidthMm: contentWidth,
      pageContentHeightMm: pageContentHeight,
    });

    for (let pageIndex = 0; pageIndex < pageSlices.length; pageIndex += 1) {
      const page = pageSlices[pageIndex];
      if (pageIndex > 0) pdf.addPage();

      const canvas = await html2canvas(surface.host, {
        scale: 2,
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

      pdf.addImage(
        canvas,
        'PNG',
        margin,
        margin,
        contentWidth,
        page.outputHeightMm,
        undefined,
        'FAST',
      );
      pdf.setFillColor(255, 255, 255);
      pdf.rect(0, margin + pageContentHeight, pageWidth, pageHeight - margin - pageContentHeight, 'F');
      addPageFooter(pdf, pageIndex + 1, pageSlices.length, title);
    }

    const arrayBuffer = pdf.output('arraybuffer');
    return new Uint8Array(arrayBuffer);
  } finally {
    surface.cleanup();
  }
}
