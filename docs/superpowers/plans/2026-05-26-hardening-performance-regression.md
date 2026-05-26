# Hardening Performance Regression Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Harden the Meeting Minutes app against XSS, render crashes, draft loss, DB bottlenecks, async blocking, and PDF/export UX regressions with repeatable tests and benchmarks.

**Architecture:** Add small focused helpers for sanitization, draft state merging, and temp-directory cleanup. Keep existing command/component boundaries, and preserve public command compatibility by adding optional pagination args rather than replacing APIs.

**Tech Stack:** React 18, Zustand, Tauri 2, Rust, rusqlite, Tokio, DOMPurify, Playwright, Node script tests, Cargo tests.

---

### Task 1: Security and Crash Regression Tests

**Files:**
- Create: `src/lib/htmlSanitizer.browser.test.mjs`
- Create: `src/components/ErrorBoundary.browser.test.mjs`
- Create: `src/components/appHardening.static.test.mjs`
- Modify: `package.json`

- [ ] Write browser regression tests proving malicious HTML event handlers/scripts are removed.
- [ ] Write browser regression tests proving a render crash shows an ErrorBoundary fallback.
- [ ] Write a static regression test proving `App.tsx` includes `ErrorBoundary` and `MinutesPreview.tsx` uses `sanitizeMinutesHtml`.
- [ ] Run each test and verify it fails before implementation.

### Task 2: Structured Minutes Draft Preservation

**Files:**
- Create: `src/pages/minutes/structuredDrafts.ts`
- Create: `src/pages/minutes/structuredDrafts.test.mjs`
- Modify: `src/pages/minutes/StructuredMinutesPanels.tsx`

- [ ] Write unit tests for preserving dirty decision/action draft fields when parent arrays are recreated.
- [ ] Run the unit test and verify it fails before implementation.
- [ ] Implement merge helpers and wire both structured panels to them.
- [ ] Rerun the unit test and verify it passes.

### Task 3: SQLite WAL, Indexes, and Pagination

**Files:**
- Modify: `src-tauri/src/commands/db.rs`
- Modify: `src-tauri/src/commands/db/meetings.rs`
- Modify: `src-tauri/src/commands/db/tests.rs`
- Modify: `src/lib/tauri.ts`

- [ ] Add Rust tests for WAL, indexes on meeting lookup tables, and paginated `get_meetings_record`.
- [ ] Run the targeted Cargo tests and verify they fail.
- [ ] Enable WAL, create indexes, and add optional `limit`/`offset` support.
- [ ] Rerun targeted Cargo tests and verify they pass.

### Task 4: Async Filesystem and Temp Cleanup

**Files:**
- Create: `src-tauri/src/commands/temp_workspace.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/commands/audio.rs`
- Modify: `src-tauri/src/commands/diarize.rs`
- Modify: `src-tauri/src/commands/transcribe.rs`
- Create: `src-tauri/src/commands/temp_workspace.rs` tests
- Create: `scripts/async_blocking_regression.test.mjs`

- [ ] Write regression tests for temp cleanup helper and selected blocking filesystem patterns.
- [ ] Run the tests and verify they fail.
- [ ] Replace selected `std::fs::create_dir_all`/`write` setup calls with `tokio::fs`.
- [ ] Clean temp worker directories after spawned workers finish.
- [ ] Rerun targeted tests.

### Task 5: Render Isolation and PDF Progress

**Files:**
- Modify: `src/components/Layout.tsx`
- Modify: `src/components/ExportButton.tsx`
- Modify: `src/lib/pdfExport.ts`
- Create: `src/components/layoutSubscriptions.static.test.mjs`
- Create: `src/lib/pdfExportProgress.static.test.mjs`

- [ ] Write regression tests proving `Layout` no longer subscribes broadly to `progress` and `ExportButton` passes progress callbacks.
- [ ] Run tests and verify they fail.
- [ ] Isolate the active processing link into a small Zustand selector component.
- [ ] Add PDF export progress callback and button label updates.
- [ ] Rerun tests.

### Task 6: Benchmarks and Full Verification

**Files:**
- Create: `scripts/runHardeningBenchmark.mjs`

- [ ] Add a lightweight benchmark for structured draft merging and pagination helper compilation.
- [ ] Run Node regression tests, targeted Cargo tests, benchmark script, TypeScript build, and targeted Playwright UI checks.
- [ ] Record any residual risk in the final response.

