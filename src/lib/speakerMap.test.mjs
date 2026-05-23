import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { rmSync, mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";

const repo = process.cwd();
const outDir = join(process.env.TEMP, "meeting-minutes-speaker-map-test");
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
      "src/lib/speakerMap.ts",
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

const {
  applySpeakerMapToText,
  extractSpeakerLabels,
  normalizeSpeakerMap,
} = require(join(outDir, "speakerMap.js"));

const labels = extractSpeakerLabels(
  ["Falante 2", "Falante 1", "Falante 2"],
  [
    { speaker: "Falante 3", start: 0, end: 1, text: "ola" },
    { speaker: "Caio", start: 1, end: 2, text: "nomeado" },
  ],
);

assert.deepEqual(labels, ["Falante 1", "Falante 2", "Falante 3", "Caio"]);

assert.deepEqual(
  normalizeSpeakerMap(["Falante 1", "Falante 2"], {
    "Falante 1": "  Caio  ",
    "Falante 2": "Falante 2",
    "Falante 9": "Fora",
  }),
  { "Falante 1": "Caio" },
);

const html = "<p><strong>Falante 1:</strong> Falante 2 respondeu ao Falante 10.</p>";
assert.equal(
  applySpeakerMapToText(html, {
    "Falante 1": "Caio",
    "Falante 2": "Emanuella",
    "Falante 10": "Rafaela",
  }),
  "<p><strong>Caio:</strong> Emanuella respondeu ao Rafaela.</p>",
);
