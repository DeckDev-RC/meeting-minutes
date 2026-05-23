# Eskuta Extraction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans while implementing this plan task-by-task. Keep the current meeting-minutes pipeline intact and only import ideas that reduce risk or add reviewability.

**Goal:** Bring the useful pieces from Eskuta into meeting-minutes without replacing the validated Groq/Cloudflare/Deepgram/local processing flow.

**Architecture:** Add small, isolated modules for secrets, evidence validation, strict insight parsing, global audio/VAD caching, and post-generation speaker mapping. Avoid another large command file.

---

### Task 1: Native Keyring

**Files:**
- Add: `src-tauri/src/commands/keyring.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/Cargo.toml`
- Modify: `src/pages/Settings.tsx`

- [ ] Store Groq, Gemini, Cloudflare token, and Deepgram key in the OS keyring.
- [ ] Keep Cloudflare account id and routing settings in `config.json`.
- [ ] Fall back to legacy `config.json` keys and migrate them when present.
- [ ] Reject unknown key names.

### Task 2: Evidence And Insight Validation

**Files:**
- Add: `src/lib/minutesEvidence.ts`
- Add: `src/lib/minutesEvidence.test.mjs`
- Modify: `src/lib/meetingFactsQueue.ts`
- Modify: `src/pages/Minutes.tsx`

- [ ] Normalize text with accent-insensitive matching.
- [ ] Validate decision/action evidence against chunk transcription.
- [ ] Sanitize malformed insight payloads before caching or rendering.
- [ ] Show verification status in the post-generation insights view.

### Task 3: Audio/VAD Fingerprint Cache

**Files:**
- Modify: `src-tauri/src/commands/audio.rs`

- [ ] Fingerprint the source audio using size plus first/last bytes.
- [ ] Store silence/VAD ranges in app data by fingerprint and detection options.
- [ ] Keep existing per-workdir silence cache as a compatible local cache.

### Task 4: Speaker Mapping UI

**Files:**
- Add: `src/lib/speakerMap.ts`
- Add: `src/lib/speakerMap.test.mjs`
- Add: `src/components/SpeakerMapPanel.tsx`
- Modify: `src-tauri/src/commands/db.rs`
- Modify: `src/lib/tauri.ts`
- Modify: `src/lib/types.ts`
- Modify: `src/pages/Minutes.tsx`

- [ ] Persist a speaker map next to the latest transcription.
- [ ] Allow renaming labels like `Falante 1` after the ata is generated.
- [ ] Apply the map to the rendered ata preview without reprocessing the meeting.

### Task 5: Verification

**Commands:**
- `node src/lib/minutesEvidence.test.mjs`
- `node src/lib/speakerMap.test.mjs`
- `cargo test --manifest-path src-tauri/Cargo.toml keyring`
- `cargo test --manifest-path src-tauri/Cargo.toml audio`
- `cargo test --manifest-path src-tauri/Cargo.toml db`
- `npm run build`

- [ ] Run focused tests.
- [ ] Run frontend build.
- [ ] Report any unresolved packaging/build risk before finalizing.
