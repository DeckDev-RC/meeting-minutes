import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { join } from "node:path";

const repo = process.cwd();
const tmpDir = join(process.env.TEMP, "meeting-minutes-text-benchmark-cli-test");
const rowsPath = join(tmpDir, "rows.json");
const outDir = join(tmpDir, "out");

rmSync(tmpDir, { recursive: true, force: true });
mkdirSync(tmpDir, { recursive: true });

function spawnNode(args, options = {}) {
  return new Promise((resolve) => {
    const child = spawn("node", args, {
      cwd: repo,
      env: process.env,
      ...options,
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => {
      stdout += chunk.toString();
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk.toString();
    });
    child.on("close", (status) => {
      resolve({ status, stdout, stderr });
    });
  });
}

writeFileSync(
  rowsPath,
  JSON.stringify({
    rows: [
      {
        row_idx: 0,
        row: {
          uid: "Boston_001",
          summary: "The committee approved the order and requested a written report.",
          transcript:
            "The chair opened the meeting. The committee approved the order. Staff will prepare a written report. Members requested follow up next week.",
        },
      },
    ],
  }),
);

const dryRun = spawnSync(
  "node",
  [
    "scripts/runTextBenchmark.mjs",
    "--dataset",
    "meetingbank",
    "--rows-file",
    rowsPath,
    "--out-dir",
    outDir,
    "--dry-run",
    "--max-cases",
    "1",
    "--target-chars",
    "200",
  ],
  { cwd: repo, encoding: "utf8" },
);

assert.equal(dryRun.status, 0, dryRun.stderr);
assert.ok(dryRun.stdout.includes("Dry run"));
assert.ok(existsSync(join(outDir, "text-benchmark-sources.json")));
assert.ok(existsSync(join(outDir, "text-benchmark-manifest-draft.json")));
assert.equal(existsSync(join(outDir, "text-benchmark-run.json")), false);

const manifest = JSON.parse(readFileSync(join(outDir, "text-benchmark-manifest-draft.json"), "utf8"));
assert.equal(manifest.cases[0].id, "meetingbank-Boston_001");
assert.deepEqual(manifest.cases[0].referenceItems, []);
assert.equal("minFactRecall" in manifest.thresholds, false);
assert.equal("minFactPrecision" in manifest.thresholds, false);

const fallbackServer = createServer((req, res) => {
  if (req.url?.startsWith("/rows")) {
    res.writeHead(500, { "content-type": "application/json" });
    res.end(JSON.stringify({ error: "viewer failed" }));
    return;
  }

  if (req.url?.startsWith("/first-rows")) {
    res.writeHead(200, { "content-type": "application/json" });
    res.end(
      JSON.stringify({
        rows: [
          {
            row_idx: 0,
            row: {
              id: 1,
              materia: "Deputados discutem energia",
              metadados: { assunto: "Energia" },
              transcricao:
                "O presidente abriu a reuniao. Maria apresentou os custos. O deputado pediu dados adicionais.",
            },
          },
        ],
      }),
    );
    return;
  }

  res.writeHead(404);
  res.end();
});
await new Promise((resolve) => fallbackServer.listen(0, "127.0.0.1", resolve));
const fallbackPort = fallbackServer.address().port;
const fallbackRun = await spawnNode(
  [
    "scripts/runTextBenchmark.mjs",
    "--dataset",
    "publichearingbr",
    "--hf-base-url",
    `http://127.0.0.1:${fallbackPort}`,
    "--out-dir",
    join(tmpDir, "fallback"),
    "--dry-run",
    "--max-cases",
    "1",
  ],
);
fallbackServer.closeAllConnections();
await new Promise((resolve) => fallbackServer.close(resolve));

assert.equal(fallbackRun.status, 0, fallbackRun.stderr);
const fallbackManifest = JSON.parse(
  readFileSync(join(tmpDir, "fallback", "text-benchmark-manifest-draft.json"), "utf8"),
);
assert.equal(fallbackManifest.cases[0].id, "publichearingbr-1");

const missingKey = spawnSync(
  "node",
  [
    "scripts/runTextBenchmark.mjs",
    "--dataset",
    "meetingbank",
    "--rows-file",
    rowsPath,
    "--out-dir",
    join(tmpDir, "missing-key"),
    "--max-cases",
    "1",
  ],
  {
    cwd: repo,
    encoding: "utf8",
    env: { ...process.env, GEMINI_API_KEY: "" },
  },
);

assert.equal(missingKey.status, 1);
assert.ok(missingKey.stderr.includes("GEMINI_API_KEY"));

const usage = spawnSync("node", ["scripts/runTextBenchmark.mjs"], {
  cwd: repo,
  encoding: "utf8",
});

assert.equal(usage.status, 1);
assert.ok(usage.stderr.includes("Usage:"));
