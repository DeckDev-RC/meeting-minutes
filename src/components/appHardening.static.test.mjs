import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const app = readFileSync("src/App.tsx", "utf8");
const preview = readFileSync("src/components/MinutesPreview.tsx", "utf8");
const packageJson = JSON.parse(readFileSync("package.json", "utf8"));

assert.match(app, /import\s+ErrorBoundary\s+from\s+["']\.\/components\/ErrorBoundary["']/);
assert.match(app, /<ErrorBoundary>/);
assert.match(app, /<\/ErrorBoundary>/);

assert.match(preview, /sanitizeMinutesHtml/);
assert.doesNotMatch(preview, /__html:\s*html/);

assert.ok(
  packageJson.dependencies?.dompurify,
  "dompurify must be a direct runtime dependency, not only a transitive lockfile entry",
);

