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
