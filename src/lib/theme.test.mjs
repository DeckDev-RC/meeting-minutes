import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { rmSync, mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";

const repo = process.cwd();
const outDir = join(process.env.TEMP, "meeting-minutes-theme-test");
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
      "src/lib/theme.ts",
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

const { nextThemePreference, normalizeThemePreference, resolveThemeMode } = require(
  join(outDir, "theme.js"),
);

assert.equal(normalizeThemePreference("dark"), "dark");
assert.equal(normalizeThemePreference("invalid"), "system");
assert.equal(resolveThemeMode("system", true), "dark");
assert.equal(resolveThemeMode("system", false), "light");
assert.equal(nextThemePreference("light"), "dark");
assert.equal(nextThemePreference("dark"), "system");
assert.equal(nextThemePreference("system"), "light");
