import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const layout = readFileSync("src/components/Layout.tsx", "utf8");

assert.match(layout, /function ProcessingNavLink/);
assert.doesNotMatch(layout, /const\s+\{\s*currentMeetingId,\s*progress,\s*stepStatus,\s*error\s*\}\s*=\s*useMeetingStore\(\)/);
assert.match(layout, /currentMeetingId\s*=\s*useMeetingStore\(\(state\)\s*=>\s*state\.currentMeetingId\)/);
assert.match(layout, /progress\s*=\s*useMeetingStore\(\(state\)\s*=>\s*state\.progress\)/);
assert.match(layout, /stepStatus\s*=\s*useMeetingStore\(\(state\)\s*=>\s*state\.stepStatus\)/);
assert.match(layout, /error\s*=\s*useMeetingStore\(\(state\)\s*=>\s*state\.error\)/);
