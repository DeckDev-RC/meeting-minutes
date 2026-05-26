import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const exportButton = readFileSync("src/components/ExportButton.tsx", "utf8");
const pdfExport = readFileSync("src/lib/pdfExport.ts", "utf8");

assert.match(pdfExport, /onProgress\?:/);
assert.match(pdfExport, /onProgress\?\.\(/);
assert.match(exportButton, /exportProgress/);
assert.match(exportButton, /onProgress:/);
assert.match(exportButton, /Exportando .*%/);
