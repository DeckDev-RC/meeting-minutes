export type PdfPageSlice = {
  sourceY: number;
  sourceHeight: number;
  outputHeightMm: number;
};

export type PdfKeepRange = {
  top: number;
  bottom: number;
  reason?: string;
};

export type PdfPageSliceInput = {
  documentHeightPx: number;
  exportWidthPx: number;
  contentWidthMm: number;
  pageContentHeightMm: number;
  keepRanges?: PdfKeepRange[];
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
const MIN_AVOIDED_SLICE_HEIGHT_RATIO = 0.35;

function normalizeKeepRanges(ranges: PdfKeepRange[] | undefined, totalHeightPx: number) {
  return (ranges ?? [])
    .map((range) => ({
      top: Math.max(0, Math.floor(range.top)),
      bottom: Math.min(totalHeightPx, Math.ceil(range.bottom)),
      reason: range.reason,
    }))
    .filter((range) => Number.isFinite(range.top) && Number.isFinite(range.bottom))
    .filter((range) => range.bottom > range.top)
    .sort((a, b) => a.top - b.top || a.bottom - b.bottom);
}

function findAvoidedPageEnd(
  pageStartPx: number,
  idealPageEndPx: number,
  pageContentHeightPx: number,
  keepRanges: PdfKeepRange[],
) {
  const minUsefulHeightPx = Math.max(
    1,
    Math.floor(pageContentHeightPx * MIN_AVOIDED_SLICE_HEIGHT_RATIO),
  );
  for (const range of keepRanges) {
    if (range.bottom <= pageStartPx) continue;
    if (range.top >= idealPageEndPx) break;
    if (idealPageEndPx <= range.top || idealPageEndPx >= range.bottom) continue;

    const candidateEnd = range.top;
    if (candidateEnd - pageStartPx >= minUsefulHeightPx) {
      return candidateEnd;
    }
  }

  return idealPageEndPx;
}

export function planPdfPageSlices({
  documentHeightPx,
  exportWidthPx,
  contentWidthMm,
  pageContentHeightMm,
  keepRanges,
}: PdfPageSliceInput): PdfPageSlice[] {
  const safeExportWidthPx = Math.max(1, Math.floor(exportWidthPx));
  const safeContentWidthMm = Math.max(1, contentWidthMm);
  const safePageContentHeightMm = Math.max(1, pageContentHeightMm);
  const pageContentHeightPx = Math.max(
    1,
    Math.floor((safePageContentHeightMm / safeContentWidthMm) * safeExportWidthPx),
  );
  const totalHeightPx = Math.max(pageContentHeightPx, Math.ceil(documentHeightPx));
  const normalizedKeepRanges = normalizeKeepRanges(keepRanges, totalHeightPx);
  const slices: PdfPageSlice[] = [];

  for (let sourceY = 0; sourceY < totalHeightPx;) {
    const idealEnd = Math.min(sourceY + pageContentHeightPx, totalHeightPx);
    const plannedEnd =
      idealEnd >= totalHeightPx
        ? idealEnd
        : findAvoidedPageEnd(sourceY, idealEnd, pageContentHeightPx, normalizedKeepRanges);
    const safeEnd = plannedEnd > sourceY ? plannedEnd : idealEnd;
    const sourceHeight = Math.max(1, Math.min(pageContentHeightPx, safeEnd - sourceY));
    slices.push({
      sourceY,
      sourceHeight,
      outputHeightMm: (sourceHeight * safeContentWidthMm) / safeExportWidthPx,
    });
    sourceY += sourceHeight;
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
