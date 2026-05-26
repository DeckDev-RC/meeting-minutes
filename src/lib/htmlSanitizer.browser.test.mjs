import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { chromium } from "@playwright/test";

const repo = process.cwd();
const outDir = join(process.env.TEMP, "meeting-minutes-html-sanitizer-test");
const entryPath = join(outDir, "entry.ts");
const bundlePath = join(outDir, "bundle.js");

rmSync(outDir, { recursive: true, force: true });
mkdirSync(outDir, { recursive: true });

writeFileSync(
  entryPath,
  `
    import { sanitizeMinutesHtml } from "${repo.replaceAll("\\", "/")}/src/lib/htmlSanitizer.ts";

    const dirty = '<section><h1>Resumo</h1><img src="x" onerror="window.__xss = true"><a href="javascript:alert(1)">bad</a><script>window.__xss = true</script></section>';
    window.__sanitizedMinutesHtml = sanitizeMinutesHtml(dirty);
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
  const sanitized = await page.evaluate(() => window.__sanitizedMinutesHtml);

  assert.match(sanitized, /<h1>Resumo<\/h1>/);
  assert.doesNotMatch(sanitized, /<script/i);
  assert.doesNotMatch(sanitized, /onerror/i);
  assert.doesNotMatch(sanitized, /javascript:/i);
  assert.equal(await page.evaluate(() => window.__xss), undefined);
} finally {
  await browser.close();
}

