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

export type PdfRenderStrategyInput = {
  documentHeightPx: number;
  exportWidthPx: number;
  scale: number;
  pageCount: number;
  maxSingleCanvasPixels?: number;
};

export type PdfRenderStrategy =
  | { mode: "single-canvas"; estimatedPixels: number }
  | { mode: "paged-canvas"; estimatedPixels: number };

const DEFAULT_MAX_SINGLE_CANVAS_PIXELS = 18_000_000;

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

export function choosePdfRenderStrategy({
  documentHeightPx,
  exportWidthPx,
  scale,
  pageCount,
  maxSingleCanvasPixels = DEFAULT_MAX_SINGLE_CANVAS_PIXELS,
}: PdfRenderStrategyInput): PdfRenderStrategy {
  const safeWidth = Math.max(1, Math.ceil(exportWidthPx));
  const safeHeight = Math.max(1, Math.ceil(documentHeightPx));
  const safeScale = Math.max(1, scale);
  const safePageCount = Math.max(1, Math.ceil(pageCount));
  const estimatedPixels = Math.ceil(safeWidth * safeHeight * safeScale * safeScale);

  if (safePageCount <= 1 || estimatedPixels <= maxSingleCanvasPixels) {
    return { mode: "single-canvas", estimatedPixels };
  }

  return { mode: "paged-canvas", estimatedPixels };
}
