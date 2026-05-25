import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { rmSync, mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";

const repo = process.cwd();
const outDir = join(process.env.TEMP, "meeting-minutes-shortcuts-test");
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
      "src/lib/shortcuts.ts",
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

const { shortcutMatches, targetAllowsGlobalShortcut } = require(join(outDir, "shortcuts.js"));

assert.equal(shortcutMatches({ key: "u", ctrlKey: true }, { key: "u", mod: true }), true);
assert.equal(shortcutMatches({ key: "U", metaKey: true }, { key: "u", mod: true }), true);
assert.equal(shortcutMatches({ key: "u" }, { key: "u", mod: true }), false);
assert.equal(targetAllowsGlobalShortcut({ tagName: "INPUT" }), false);
assert.equal(targetAllowsGlobalShortcut({ tagName: "DIV" }), true);
