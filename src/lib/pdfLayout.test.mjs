import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { rmSync, mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";

const repo = process.cwd();
const outDir = join(process.env.TEMP, "meeting-minutes-pdf-layout-test");
const require = createRequire(import.meta.url);

rmSync(outDir, { recursive: true, force: true });
mkdirSync(outDir, { recursive: true });

const compile = spawnSync(
  "cmd.exe",
  [
    "/d",
    "/s",
    "/c",
    [
      "npx",
      "tsc",
      "src/lib/pdfLayout.ts",
      "--target",
      "ES2021",
      "--module",
      "CommonJS",
      "--moduleResolution",
      "node",
      "--skipLibCheck",
      "--outDir",
      outDir,
    ].join(" "),
  ],
  { cwd: repo, encoding: "utf8" },
);

assert.equal(compile.status, 0, compile.stdout + compile.stderr);

const { choosePdfRenderStrategy, planPdfPageSlices } = require(join(outDir, "pdfLayout.js"));

const slices = planPdfPageSlices({
  documentHeightPx: 2400,
  exportWidthPx: 800,
  contentWidthMm: 160,
  pageContentHeightMm: 200,
});

assert.deepEqual(slices, [
  { sourceY: 0, sourceHeight: 1000, outputHeightMm: 200 },
  { sourceY: 1000, sourceHeight: 1000, outputHeightMm: 200 },
  { sourceY: 2000, sourceHeight: 400, outputHeightMm: 80 },
]);

assert.deepEqual(
  planPdfPageSlices({
    documentHeightPx: 0,
    exportWidthPx: 800,
    contentWidthMm: 160,
    pageContentHeightMm: 200,
  }),
  [{ sourceY: 0, sourceHeight: 1000, outputHeightMm: 200 }],
);

assert.deepEqual(
  choosePdfRenderStrategy({
    documentHeightPx: 2400,
    exportWidthPx: 800,
    scale: 2,
    pageCount: 3,
    maxSingleCanvasPixels: 8_000_000,
  }),
  { mode: "single-canvas", estimatedPixels: 7_680_000 },
);

assert.deepEqual(
  choosePdfRenderStrategy({
    documentHeightPx: 12_000,
    exportWidthPx: 800,
    scale: 2,
    pageCount: 12,
    maxSingleCanvasPixels: 8_000_000,
  }),
  { mode: "paged-canvas", estimatedPixels: 38_400_000 },
);

assert.deepEqual(
  planPdfPageSlices({
    documentHeightPx: 2400,
    exportWidthPx: 800,
    contentWidthMm: 160,
    pageContentHeightMm: 200,
    keepRanges: [{ top: 920, bottom: 1160, reason: "decision-card" }],
  }),
  [
    { sourceY: 0, sourceHeight: 920, outputHeightMm: 184 },
    { sourceY: 920, sourceHeight: 1000, outputHeightMm: 200 },
    { sourceY: 1920, sourceHeight: 480, outputHeightMm: 96 },
  ],
);

assert.deepEqual(
  planPdfPageSlices({
    documentHeightPx: 1800,
    exportWidthPx: 800,
    contentWidthMm: 160,
    pageContentHeightMm: 200,
    keepRanges: [
      { top: 820, bottom: 980, reason: "decision-card-2" },
      { top: 980, bottom: 1140, reason: "decision-card-3" },
    ],
  }),
  [
    { sourceY: 0, sourceHeight: 980, outputHeightMm: 196 },
    { sourceY: 980, sourceHeight: 820, outputHeightMm: 164 },
  ],
);

assert.deepEqual(
  planPdfPageSlices({
    documentHeightPx: 2400,
    exportWidthPx: 800,
    contentWidthMm: 160,
    pageContentHeightMm: 200,
    keepRanges: [
      { top: 990, bottom: 1040, reason: "table-row" },
      { top: 1960, bottom: 2010, reason: "table-row" },
    ],
  }),
  [
    { sourceY: 0, sourceHeight: 990, outputHeightMm: 198 },
    { sourceY: 990, sourceHeight: 970, outputHeightMm: 194 },
    { sourceY: 1960, sourceHeight: 440, outputHeightMm: 88 },
  ],
);
