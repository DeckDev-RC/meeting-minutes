# Adaptive Transcription Router Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an adaptive transcription architecture that keeps Groq available at peak behavior, uses Cloudflare as the low-cost cloud default, uses Deepgram as an explicit/high-quality option, and preserves local Parakeet/faster-whisper fallback.

**Architecture:** Add cloud provider commands in Rust behind the existing `transcribe.rs` boundary, then route provider selection in `src/lib/transcriptionProvider.ts`. Keep the existing Processing pipeline shape by treating Groq, Cloudflare, and Deepgram as remote chunk transcribers and local engines as batch transcribers.

**Tech Stack:** Tauri Rust commands, reqwest multipart/JSON APIs, React/TypeScript settings and processing UI, existing benchmark artifacts.

---

### Task 1: Provider Selection Model

**Files:**
- Modify: `src/lib/types.ts`
- Modify: `src/lib/transcriptionProvider.ts`
- Test: `src/lib/transcriptionProvider.test.mjs`

- [ ] Add transcription profiles: `smart-low-cost`, `max-quality`, `groq-turbo`, `offline-free`, `manual`.
- [ ] Add provider backend ids: `cloudflare`, `deepgram`, `groq`, `parakeet-local`, `local`.
- [ ] Make `smart-low-cost` choose Cloudflare when configured, Groq when Cloudflare is missing and Groq exists, otherwise Parakeet for long audio or faster-whisper local.
- [ ] Make `max-quality` choose Deepgram when configured, otherwise Cloudflare, then Groq, then local.
- [ ] Make `groq-turbo` preserve Groq whenever a key exists.
- [ ] Keep xAI out of active routing.

### Task 2: Rust Cloud Transcription Commands

**Files:**
- Modify: `src-tauri/src/commands/transcribe.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/lib/tauri.ts`

- [ ] Add response parsers for Cloudflare and Deepgram.
- [ ] Add `transcribe_chunk_cloudflare` and `transcribe_chunk_deepgram` commands.
- [ ] Add retry/error handling similar to Groq for rate limits and 5xx.
- [ ] Preserve Groq command unchanged.

### Task 3: Settings UI

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/lib/tauri.ts`
- Modify: `src/pages/Settings.tsx`

- [ ] Store Cloudflare account id/token, Deepgram key, transcription profile, and optional manual provider.
- [ ] Show profile choices with concise labels and operational tradeoffs.
- [ ] Keep Groq and Gemini fields intact.

### Task 4: Processing Integration

**Files:**
- Modify: `src/pages/Processing.tsx`
- Modify: `src/lib/localTranscription.ts`

- [ ] Load transcription settings from `getApiKeys`.
- [ ] Select backend using the new profile-aware router.
- [ ] For remote providers, call the matching command through the existing concurrent queue.
- [ ] For local providers, keep the existing batch path.
- [ ] Log provider label and preserve progress behavior.

### Task 5: Verification

**Commands:**
- `node src/lib/transcriptionProvider.test.mjs`
- `cargo test --manifest-path src-tauri/Cargo.toml transcribe`
- `npm run build`

- [ ] Run unit tests for router behavior.
- [ ] Run Rust tests for parsers and retry policy.
- [ ] Run frontend build.
- [ ] Do not create installers or push unless explicitly requested.
