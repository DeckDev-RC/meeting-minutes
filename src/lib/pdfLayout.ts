export type PdfPageSlice = {
  sourceY: number;
  sourceHeight: number;
  outputHeightMm: number;
};

export type PdfPageSliceInput = {
  documentHeightPx: number;
  exportWidthPx: number;
  contentWidthMm: number;
  pageContentHeightMm: number;
};

export function planPdfPageSlices({
  documentHeightPx,
  exportWidthPx,
  contentWidthMm,
  pageContentHeightMm,
}: PdfPageSliceInput): PdfPageSlice[] {
  const safeExportWidthPx = Math.max(1, Math.floor(exportWidthPx));
  const safeContentWidthMm = Math.max(1, contentWidthMm);
  const safePageContentHeightMm = Math.max(1, pageContentHeightMm);
  const pageContentHeightPx = Math.max(
    1,
    Math.floor((safePageContentHeightMm / safeContentWidthMm) * safeExportWidthPx),
  );
  const totalHeightPx = Math.max(pageContentHeightPx, Math.ceil(documentHeightPx));
  const slices: PdfPageSlice[] = [];

  for (let sourceY = 0; sourceY < totalHeightPx; sourceY += pageContentHeightPx) {
    const sourceHeight = Math.min(pageContentHeightPx, totalHeightPx - sourceY);
    slices.push({
      sourceY,
      sourceHeight,
      outputHeightMm: (sourceHeight * safeContentWidthMm) / safeExportWidthPx,
    });
  }

  return slices;
}
