import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { chromium } from "@playwright/test";

const repo = process.cwd();
const outDir = join(process.env.TEMP, "meeting-minutes-error-boundary-test");
const entryPath = join(outDir, "entry.tsx");
const bundlePath = join(outDir, "bundle.js");

rmSync(outDir, { recursive: true, force: true });
mkdirSync(outDir, { recursive: true });

writeFileSync(
  entryPath,
  `
    import React from "${repo.replaceAll("\\", "/")}/node_modules/react/index.js";
    import { createRoot } from "${repo.replaceAll("\\", "/")}/node_modules/react-dom/client.js";
    import ErrorBoundary from "${repo.replaceAll("\\", "/")}/src/components/ErrorBoundary.tsx";

    function Broken() {
      throw new Error("render failed");
    }

    createRoot(document.getElementById("root")!).render(
      <ErrorBoundary>
        <Broken />
      </ErrorBoundary>
    );
  `,
);

const bundle = spawnSync(
  "cmd.exe",
  [
    "/d",
    "/s",
    "/c",
    [
      "npx",
      "esbuild",
      entryPath,
      "--bundle",
      "--format=iife",
      "--platform=browser",
      "--jsx=automatic",
      `--alias:react=${repo.replaceAll("\\", "/")}/node_modules/react/index.js`,
      `--alias:react/jsx-runtime=${repo.replaceAll("\\", "/")}/node_modules/react/jsx-runtime.js`,
      `--alias:react-dom/client=${repo.replaceAll("\\", "/")}/node_modules/react-dom/client.js`,
      `--outfile=${bundlePath}`,
    ].join(" "),
  ],
  { cwd: repo, encoding: "utf8" },
);

assert.equal(bundle.status, 0, bundle.stdout + bundle.stderr);

const browser = await chromium.launch();
try {
  const page = await browser.newPage();
  await page.setContent(`<div id="root"></div><script>${readFileSync(bundlePath, "utf8")}</script>`);
  await page.getByRole("alert").waitFor();
  const text = await page.getByRole("alert").innerText();

  assert.match(text, /Algo deu errado/i);
  assert.match(text, /Recarregar/i);
} finally {
  await browser.close();
}
