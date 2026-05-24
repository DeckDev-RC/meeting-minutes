# Structured Minutes And Processing Jobs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Persist meeting minutes as structured data with evidence validation and processing job telemetry while keeping the current HTML/PDF workflow working.

**Architecture:** Keep the current Tauri/Rust pipeline as the source of truth. Build structured records from existing `MeetingChunkInsights` at save time, persist them in new SQLite tables, and expose them through new read commands. Refactor the React processing screen by extracting pure helpers/components without changing the execution order.

**Tech Stack:** Tauri 2, Rust, rusqlite, serde, React 18, TypeScript, current local benchmark/test scripts.

---

### Task 1: Structured Minutes Schema And Persistence

**Files:**
- Modify: `src-tauri/src/commands/db.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/lib/tauri.ts`
- Modify: `src/lib/types.ts`

- [x] Add SQLite tables for `minute_versions`, `minute_decisions`, `minute_actions`, and `minute_evidences`.
- [x] Extend `save_minutes` with optional `facts_json`, `diarized_json`, and `participant_names`.
- [x] Derive structured decisions/actions/evidences from facts without changing the existing HTML storage.
- [x] Add tests that verify new DBs and migrated DBs get the new tables.
- [x] Add tests that structured persistence inserts rows when facts are provided.

### Task 2: Backend Evidence Validation

**Files:**
- Create: `src-tauri/src/commands/minutes_validator.rs`
- Modify: `src-tauri/src/commands/db.rs`
- Modify: `src-tauri/src/commands/mod.rs`

- [x] Implement normalized substring/fuzzy-enough evidence matching without adding a heavy dependency.
- [x] Store validation status and score in `minute_evidences`.
- [x] Add tests for exact evidence, normalized evidence, weak evidence, and empty evidence.

### Task 3: Processing Jobs

**Files:**
- Modify: `src-tauri/src/commands/db.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/lib/tauri.ts`
- Modify: `src/lib/types.ts`
- Modify: `src/pages/Processing.tsx`

- [x] Add `processing_jobs` table with stage, status, progress, error, started/finished timestamps.
- [x] Add commands `upsert_processing_job` and `get_processing_jobs`.
- [x] Call `upsert_processing_job` from major pipeline progress updates in `Processing.tsx`.
- [x] Add DB tests for job upsert and retrieval.

### Task 4: Processing.tsx Refactor

**Files:**
- Create: `src/pages/processing/utils.ts`
- Create: `src/pages/processing/LiveProcessingPanel.tsx`
- Modify: `src/pages/Processing.tsx`

- [x] Move pure processing helpers to `processing/utils.ts`.
- [x] Move the live tab panel rendering to `LiveProcessingPanel.tsx`.
- [x] Keep the pipeline orchestration in `Processing.tsx`.
- [x] Run TypeScript build and e2e smoke tests.

### Task 5: Medium-Priority Architecture Hooks

**Files:**
- Create: `docs/architecture/sidecar-runtime.md`
- Create: `src-tauri/src/commands/transcription_router.rs`
- Create: `src-tauri/src/commands/minutes_pipeline.rs`
- Create: `src-tauri/src/commands/audio_chunker.rs`

- [x] Add thin Rust modules that wrap current commands and define boundaries for later deeper migration.
- [x] Document the single-installer sidecar strategy, including why the current bundled runtime stays until benchmark parity is proven.
- [x] Do not move runtime implementation until benchmarks justify it.

### Verification

- [x] `cargo test --manifest-path src-tauri/Cargo.toml`
- [x] `npm run build`
- [x] Existing e2e smoke tests that do not need paid API keys.
