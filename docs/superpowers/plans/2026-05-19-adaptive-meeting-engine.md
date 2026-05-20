# Adaptive Meeting Engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Convert the meeting processing flow from a linear pipeline into a free-first adaptive pipeline that overlaps diarization, transcription, and fact extraction.

**Architecture:** Split diarization into two phases: local speaker-turn detection and transcript alignment. Start speaker-turn detection as soon as audio exists, transcribe chunks in parallel, extract facts from transcript chunks without waiting for speaker alignment, then generate final minutes after both facts and diarized transcript are available.

**Tech Stack:** Rust/Tauri 2 commands, React/TypeScript processing screen, existing `diarize` CPU backend, Gemini facts/minutes, Groq transcription.

---

### Task 1: Speaker Turn API

**Files:**
- Modify: `src-tauri/src/commands/diarize.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/lib/types.ts`
- Modify: `src/lib/tauri.ts`

- [x] **Step 1: Add failing tests for serializable speaker turns**
- [x] **Step 2: Run tests and verify compile failure**
- [x] **Step 3: Derive serde for `SpeakerTurn` and expose camelCase JSON**
- [x] **Step 4: Add `diarize_audio_turns_modern_cpu` command**
- [x] **Step 5: Add `align_speaker_turns_to_transcription` command**
- [x] **Step 6: Register commands and TypeScript wrappers**
- [x] **Step 7: Run focused Rust tests**

### Task 2: Adaptive Frontend Pipeline

**Files:**
- Modify: `src/pages/Processing.tsx`
- Modify: `src/components/ProgressPipeline.tsx`
- Modify: `src/lib/pipelineProgress.ts`
- Modify: `src/lib/pipelineProgress.test.mjs`
- Modify: `src/pages/Settings.tsx`
- Modify: `src-tauri/src/lib.rs`

- [x] **Step 1: Start speaker-turn diarization after chunks exist**
- [x] **Step 2: Keep transcribing chunks while diarization runs**
- [x] **Step 3: Start fact extraction from transcript chunks without waiting for diarization**
- [x] **Step 4: Align speaker turns to transcript when both are available**
- [x] **Step 5: Fall back to existing end-to-end diarization if speculative turns are unavailable**
- [x] **Step 6: Update progress copy to explain parallel work**
- [x] **Step 7: Add optional expected speaker count setting**
- [x] **Step 8: Pass expected speaker count into speculative and fallback diarization**
- [x] **Step 9: Run TypeScript build and progress helper test**

### Task 3: Adaptive E2E Benchmark

**Files:**
- Modify: `src-tauri/src/bin/e2e_benchmark.rs`
- Modify: `docs/evaluation/free-benchmark-protocol.md`

- [x] **Step 1: Start speculative speaker-turn detection before transcription**
- [x] **Step 2: Extract facts from transcript chunks in parallel with final diarization alignment**
- [x] **Step 3: Preserve fallback path for `auto` diarization mode**
- [x] **Step 4: Record overlapping stages as `diarize_speculative` and `extract_facts_parallel`**
- [x] **Step 5: Run real AMI ES2002a E2E benchmark**
- [x] **Step 6: Document benchmark numbers**

### Task 4: Final Verification

**Files:**
- Verify all modified code.

- [x] **Step 1: `cargo test --lib commands::diarize::tests:: -- --nocapture`**
- [x] **Step 2: `node src/lib/pipelineProgress.test.mjs`**
- [x] **Step 3: `npm run build`**
- [x] **Step 4: `cargo check --bin meeting-minutes`**
- [x] **Step 5: `cargo check --bin e2e_benchmark`**
- [x] **Step 6: `cargo check --bin diarization_benchmark`**
- [x] **Step 7: Real E2E benchmark: `118.45s`, RTF `0.093`, `10.74x`**

### Task 5: Selective Second Pass

**Files:**
- Modify: `src-tauri/src/commands/diarize.rs`
- Modify: `src-tauri/src/bin/e2e_benchmark.rs`
- Modify: `src/lib/tauri.ts`
- Modify: `src/pages/Processing.tsx`
- Modify: `docs/evaluation/free-benchmark-protocol.md`

- [x] **Step 1: Add failing tests for suspicious chunk/window selection**
- [x] **Step 2: Add failing test for selective merge**
- [x] **Step 3: Implement suspicion scoring from weak turn coverage and ambiguous overlap**
- [x] **Step 4: Implement short refinement windows with padding and max duration**
- [x] **Step 5: Add Tauri command `refine_diarization_selectively`**
- [x] **Step 6: Plug selective refinement into the adaptive Processing flow**
- [x] **Step 7: Mirror the selective path in the E2E benchmark runner**
- [x] **Step 8: Benchmark Sherpa selective and reject it for the fast path**
- [x] **Step 9: Switch the fast selective path to modern CPU sub-window reprocessing**
- [x] **Step 10: Benchmark modern selective path: `150.85s`, RTF `0.119`, `8.44x`**
