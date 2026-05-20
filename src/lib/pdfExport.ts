import jsPDF from 'jspdf';
import html2canvas from 'html2canvas';

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

    const canvas = await html2canvas(surface.host, {
      scale: 2,
      useCORS: true,
      backgroundColor: '#ffffff',
      width: EXPORT_WIDTH_PX,
      windowWidth: EXPORT_WIDTH_PX,
      scrollX: 0,
      scrollY: 0,
    });

    const imgData = canvas.toDataURL('image/png');
    const imgHeight = (canvas.height * contentWidth) / canvas.width;

    const pageContentHeight = pageHeight - margin * 2 - footerSpace;
    const pageCount = Math.max(1, Math.ceil(imgHeight / pageContentHeight));

    for (let page = 0; page < pageCount; page += 1) {
      if (page > 0) pdf.addPage();

      pdf.addImage(
        imgData,
        'PNG',
        margin,
        margin - page * pageContentHeight,
        contentWidth,
        imgHeight,
        undefined,
        'FAST',
      );
      pdf.setFillColor(255, 255, 255);
      pdf.rect(0, margin + pageContentHeight, pageWidth, pageHeight - margin - pageContentHeight, 'F');
      addPageFooter(pdf, page + 1, pageCount, title);
    }

    const arrayBuffer = pdf.output('arraybuffer');
    return new Uint8Array(arrayBuffer);
  } finally {
    surface.cleanup();
  }
}
