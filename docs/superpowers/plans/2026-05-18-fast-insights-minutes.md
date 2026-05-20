# Fast Insights Minutes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reduce the bottleneck in steps 3 and 4 by extracting reusable meeting facts per chunk and generating the final minutes from compact structured facts.

**Architecture:** Keep the existing transcription and Gemini integrations, but stop sending the whole meeting transcript directly to the final minutes prompt. Store per-chunk facts in `processing_chunks`, reuse them on resume, then call Gemini once with compact facts plus diarized speaker context.

**Tech Stack:** Tauri 2, Rust, rusqlite, React, TypeScript, Gemini 2.5 Flash, Node-based TS smoke tests, Cargo unit tests.

---

### Task 1: Meeting Insight Types And Prompt Builders

**Files:**
- Modify: `src-tauri/src/models/transcription.rs`
- Modify: `src-tauri/src/commands/generate.rs`

- [x] **Step 1: Write failing Rust tests for compact fact extraction helpers**

Test that chunk insight prompt input can be parsed into typed structs and that compact agenda input preserves decisions/actions without needing raw transcript.

- [x] **Step 2: Run Rust tests to verify they fail**

Run: `cargo test --lib generate::tests`

Expected: compile failure because the new helpers do not exist yet.

- [x] **Step 3: Implement typed meeting insight structs and prompt helpers**

Add `MeetingChunkInsights`, `MeetingDecision`, and `MeetingAction` using camelCase serde names. Store open questions as compact strings. Add prompt/payload helper functions in `generate.rs`.

- [x] **Step 4: Run Rust tests to verify they pass**

Run: `cargo test --lib generate::tests`

Expected: PASS.

### Task 2: Cache Chunk Facts In SQLite

**Files:**
- Modify: `src-tauri/src/models/audio.rs`
- Modify: `src-tauri/src/commands/db.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/lib/types.ts`
- Modify: `src/lib/tauri.ts`

- [x] **Step 1: Add columns for chunk facts**

Add `facts_status`, `facts_json`, and `facts_error_msg` to `processing_chunks`, with migration for existing user databases.

- [x] **Step 2: Add update command**

Expose `update_processing_chunk_facts` to mark per-chunk fact extraction as `pending`, `running`, `done`, or `error`.

- [x] **Step 3: Include fact fields in chunk records**

Return cached fact status and JSON in both Rust and TypeScript models.

### Task 3: Concurrent Fact Extraction Queue

**Files:**
- Create: `src/lib/meetingFactsQueue.ts`
- Create: `src/lib/meetingFactsQueue.test.mjs`
- Modify: `src/lib/tauri.ts`

- [x] **Step 1: Write failing TypeScript smoke test**

Test that pending chunks are extracted concurrently, cached chunks are reused, progress events fire, and failures do not get marked done.

- [x] **Step 2: Run test to verify it fails**

Run: `node src/lib/meetingFactsQueue.test.mjs`

Expected: TypeScript compile failure or missing export.

- [x] **Step 3: Implement the queue**

Create `extractMeetingFactsConcurrently` with bounded concurrency and stable output ordering by chunk index.

- [x] **Step 4: Run test to verify it passes**

Run: `node src/lib/meetingFactsQueue.test.mjs`

Expected: PASS.

### Task 4: Wire Processing Pipeline

**Files:**
- Modify: `src/pages/Processing.tsx`
- Modify: `src/lib/pipelineProgress.ts`
- Modify: `src/lib/pipelineProgress.test.mjs`

- [x] **Step 1: Extract chunk facts after transcription**

After all chunks are transcribed, run fact extraction only for chunks without cached facts. Keep concurrency conservative to avoid free-tier pressure.

- [x] **Step 2: Generate final minutes from facts**

Replace the final whole-transcript minutes call with `generateAtaFromFacts`.

- [x] **Step 3: Improve user-facing progress text**

Show that the app is extracting decisions/actions and composing the final document instead of a vague loading state.

### Task 5: Verification

**Files:**
- All touched files.

- [x] **Step 1: Run focused tests**

Run: `node src/lib/meetingFactsQueue.test.mjs`, `node src/lib/pipelineProgress.test.mjs`, and `cargo test --lib`.

- [x] **Step 2: Run production build**

Run: `npm run build`.

- [x] **Step 3: Run Tauri debug build**

Run: `npm run tauri -- build --debug`.

### Task 6: End-To-End Offline Diarization Path

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/commands/diarize.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/lib/tauri.ts`
- Modify: `src/pages/Processing.tsx`
- Modify: `src/lib/pipelineProgress.ts`
- Modify: `src/components/ProgressPipeline.tsx`

- [x] **Step 1: Add tests for audio turn alignment and model asset paths**

Run: `cargo test --lib diarize::tests`

Expected: initial failure before implementation, then PASS after implementation.

- [x] **Step 2: Add sherpa-onnx dependency and local model manager**

Add `sherpa-onnx = "1.13.2"` and download free local models into the app data directory when missing.

- [x] **Step 3: Add end-to-end diarization command**

Add `diarize_transcription_end_to_end`, which reads WAV audio, runs sherpa-onnx speaker diarization, aligns acoustic turns to transcript segments, and falls back to the fast local heuristic if model setup or inference fails.

- [x] **Step 4: Wire Processing to the new command**

Use a 16 kHz mono WAV extraction target and call `diarizeTranscriptionEndToEnd(audioOutput, segmentsJson)`.

- [x] **Step 5: Verify builds**

Run: `cargo test --lib`, `node src/lib/pipelineProgress.test.mjs`, `node src/lib/meetingFactsQueue.test.mjs`, `node src/lib/transcriptionQueue.test.mjs`, `npm run build`, and `npm run tauri -- build --debug`.
