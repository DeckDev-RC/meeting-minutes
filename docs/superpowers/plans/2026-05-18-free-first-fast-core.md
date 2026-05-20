# Free-First Fast Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Build the free-first fast core: FLAC/WAV extraction, silence-aware chunking, concurrent transcription, chunk-level resume, and progress based on processed audio seconds.

**Architecture:** Keep heavy audio work in Tauri/Rust using the bundled FFmpeg. Keep orchestration, concurrency, and UI progress in TypeScript. Persist chunk state in SQLite so processing can resume without restarting completed transcription chunks.

**Tech Stack:** Tauri 2, Rust, rusqlite, FFmpeg sidecar, React, TypeScript, Zustand, Node test runner via `tsc`-compiled helper tests, Cargo tests.

---

## Scope

This plan implements Phase 1 from `docs/superpowers/specs/2026-05-18-free-first-fast-transcription-design.md`.

Included:

- Extract source media to `flac` by default.
- Detect silence locally with FFmpeg.
- Build smart chunk plans around silence.
- Export chunks with overlap.
- Transcribe chunks concurrently with bounded retries and rate-limit backoff.
- Persist chunk status/results.
- Resume transcription from completed chunks.
- Show chunk count, ETA, speed factor, and audio-seconds progress.

Deferred to later plans:

- Metadata quality scoring.
- Selective high-accuracy retry.
- Map-reduce minutes generation.
- Optional pyannote/Silero diarization.

## Repository Note

`c:\C\pop\meeting-minutes` is currently not a Git repository. If execution happens before Git is initialized, skip commit commands and provide a per-task change summary instead. If Git is initialized before execution, run the commit commands exactly as written.

## File Structure

Create:

- `src/lib/transcriptionQueue.ts`: TypeScript concurrency queue for chunk transcription.
- `src/lib/transcriptionQueue.test.mjs`: Node-based test that compiles and validates `transcriptionQueue.ts`.
- `src/lib/pipelineProgress.ts`: derives processing UI stats from chunk progress.
- `src/lib/pipelineProgress.test.mjs`: Node-based test that compiles and validates progress math.
- `src-tauri/src/models/audio.rs`: Rust structs shared by audio and DB commands.

Modify:

- `src-tauri/src/models/mod.rs`: export the new `audio` module.
- `src-tauri/src/commands/audio.rs`: add pure silence parsing/chunk planning helpers plus FFmpeg commands.
- `src-tauri/src/commands/db.rs`: add `processing_chunks` table and chunk persistence commands.
- `src-tauri/src/lib.rs`: register new Tauri commands.
- `src/lib/types.ts`: add audio chunk/result types.
- `src/lib/tauri.ts`: add wrappers for new commands.
- `src/lib/processingProgress.ts`: add richer progress stages.
- `src/store/meetingStore.ts`: add progress detail fields for chunk count, ETA, speed factor.
- `src/pages/Processing.tsx`: replace sequential chunk transcription with smart chunk pipeline and resume.

## Task 1: Rust Audio Models

**Files:**

- Create: `src-tauri/src/models/audio.rs`
- Modify: `src-tauri/src/models/mod.rs`

- [x] **Step 1: Create audio model structs**

Create `src-tauri/src/models/audio.rs` with:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SilenceRange {
    pub start_sec: f64,
    pub end_sec: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChunkPlan {
    pub index: usize,
    pub start_sec: f64,
    pub end_sec: f64,
    pub offset_sec: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExportedChunk {
    pub index: usize,
    pub audio_path: String,
    pub start_sec: f64,
    pub end_sec: f64,
    pub offset_sec: f64,
    pub duration_sec: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SmartChunkOptions {
    pub target_sec: f64,
    pub min_sec: f64,
    pub max_sec: f64,
    pub overlap_sec: f64,
    pub silence_min_duration_sec: f64,
    pub silence_noise_db: f64,
    pub output_format: String,
}

impl Default for SmartChunkOptions {
    fn default() -> Self {
        Self {
            target_sec: 360.0,
            min_sec: 180.0,
            max_sec: 480.0,
            overlap_sec: 3.0,
            silence_min_duration_sec: 0.45,
            silence_noise_db: -35.0,
            output_format: "flac".to_string(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProcessingChunkRecord {
    pub meeting_id: String,
    pub index: usize,
    pub audio_path: String,
    pub start_sec: f64,
    pub end_sec: f64,
    pub offset_sec: f64,
    pub duration_sec: f64,
    pub status: String,
    pub raw_segments_json: Option<String>,
    pub error_msg: Option<String>,
}
```

- [x] **Step 2: Export the module**

Modify `src-tauri/src/models/mod.rs` to:

```rust
pub mod audio;
pub mod meeting;
pub mod transcription;
```

- [x] **Step 3: Verify Rust module compiles**

Run:

```powershell
cargo check
```

from `src-tauri`.

Expected: exit code `0`.

- [x] **Step 4: Commit**

If Git exists:

```powershell
git add src-tauri/src/models/audio.rs src-tauri/src/models/mod.rs
git commit -m "feat: add audio chunk models"
```

## Task 2: Silence Parsing and Chunk Planning

**Files:**

- Modify: `src-tauri/src/commands/audio.rs`

- [x] **Step 1: Add failing Rust tests for silence parsing and chunk planning**

Append this test module to the bottom of `src-tauri/src/commands/audio.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::audio::SilenceRange;

    #[test]
    fn parses_ffmpeg_silencedetect_ranges() {
        let stderr = "
            [silencedetect @ 000] silence_start: 12.345
            [silencedetect @ 000] silence_end: 14.890 | silence_duration: 2.545
            [silencedetect @ 000] silence_start: 44
            [silencedetect @ 000] silence_end: 45.25 | silence_duration: 1.25
        ";

        let ranges = parse_silencedetect(stderr);

        assert_eq!(
            ranges,
            vec![
                SilenceRange { start_sec: 12.345, end_sec: 14.890 },
                SilenceRange { start_sec: 44.0, end_sec: 45.25 },
            ]
        );
    }

    #[test]
    fn plans_chunks_near_silence_with_overlap() {
        let silences = vec![
            SilenceRange { start_sec: 295.0, end_sec: 300.0 },
            SilenceRange { start_sec: 602.0, end_sec: 606.0 },
        ];

        let chunks = plan_smart_chunks(900.0, &silences, 300.0, 180.0, 360.0, 3.0);

        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].index, 0);
        assert_eq!(chunks[0].start_sec, 0.0);
        assert_eq!(chunks[0].end_sec, 297.5);
        assert_eq!(chunks[0].offset_sec, 0.0);
        assert_eq!(chunks[1].start_sec, 294.5);
        assert_eq!(chunks[1].end_sec, 604.0);
        assert_eq!(chunks[1].offset_sec, 294.5);
        assert_eq!(chunks[2].start_sec, 601.0);
        assert_eq!(chunks[2].end_sec, 900.0);
    }

    #[test]
    fn falls_back_to_time_chunks_without_silence() {
        let chunks = plan_smart_chunks(650.0, &[], 300.0, 180.0, 360.0, 3.0);

        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].start_sec, 0.0);
        assert_eq!(chunks[0].end_sec, 300.0);
        assert_eq!(chunks[1].start_sec, 297.0);
        assert_eq!(chunks[1].end_sec, 597.0);
        assert_eq!(chunks[2].start_sec, 594.0);
        assert_eq!(chunks[2].end_sec, 650.0);
    }
}
```

- [x] **Step 2: Run tests to verify they fail**

Run:

```powershell
cargo test commands::audio::tests --lib
```

from `src-tauri`.

Expected: fail because `parse_silencedetect` and `plan_smart_chunks` do not exist.

- [x] **Step 3: Add imports and helper implementations**

At the top of `src-tauri/src/commands/audio.rs`, add:

```rust
use crate::models::audio::{ChunkPlan, ExportedChunk, SilenceRange, SmartChunkOptions};
use std::path::Path;
```

Below `command_output_error`, add:

```rust
fn parse_silencedetect(stderr: &str) -> Vec<SilenceRange> {
    let mut ranges = Vec::new();
    let mut current_start: Option<f64> = None;

    for line in stderr.lines() {
        if let Some(raw) = line.split("silence_start:").nth(1) {
            current_start = raw.trim().split_whitespace().next().and_then(|v| v.parse::<f64>().ok());
        }

        if let Some(raw) = line.split("silence_end:").nth(1) {
            if let Some(start_sec) = current_start.take() {
                if let Some(end_sec) = raw.trim().split_whitespace().next().and_then(|v| v.parse::<f64>().ok()) {
                    if end_sec > start_sec {
                        ranges.push(SilenceRange { start_sec, end_sec });
                    }
                }
            }
        }
    }

    ranges
}

fn midpoint(range: &SilenceRange) -> f64 {
    (range.start_sec + range.end_sec) / 2.0
}

fn find_cut_point(
    desired_end: f64,
    min_end: f64,
    max_end: f64,
    silences: &[SilenceRange],
) -> f64 {
    silences
        .iter()
        .map(midpoint)
        .filter(|point| *point >= min_end && *point <= max_end)
        .min_by(|a, b| {
            let da = (*a - desired_end).abs();
            let db = (*b - desired_end).abs();
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(desired_end.min(max_end))
}

fn plan_smart_chunks(
    duration_sec: f64,
    silences: &[SilenceRange],
    target_sec: f64,
    min_sec: f64,
    max_sec: f64,
    overlap_sec: f64,
) -> Vec<ChunkPlan> {
    if duration_sec <= 0.0 {
        return Vec::new();
    }

    let mut chunks = Vec::new();
    let mut start_sec = 0.0;
    let mut index = 0usize;

    while start_sec < duration_sec {
        let remaining = duration_sec - start_sec;
        let end_sec = if remaining <= max_sec {
            duration_sec
        } else {
            let desired_end = (start_sec + target_sec).min(duration_sec);
            let min_end = (start_sec + min_sec).min(duration_sec);
            let max_end = (start_sec + max_sec).min(duration_sec);
            find_cut_point(desired_end, min_end, max_end, silences)
        };

        chunks.push(ChunkPlan {
            index,
            start_sec,
            end_sec,
            offset_sec: start_sec,
        });

        if end_sec >= duration_sec {
            break;
        }

        start_sec = (end_sec - overlap_sec).max(0.0);
        index += 1;
    }

    chunks
}
```

- [x] **Step 4: Run tests to verify they pass**

Run:

```powershell
cargo test commands::audio::tests --lib
```

Expected: all three tests pass.

- [x] **Step 5: Commit**

If Git exists:

```powershell
git add src-tauri/src/commands/audio.rs
git commit -m "feat: plan silence-aware audio chunks"
```

## Task 3: FFmpeg Smart Chunk Commands

**Files:**

- Modify: `src-tauri/src/commands/audio.rs`
- Modify: `src-tauri/src/lib.rs`

- [x] **Step 1: Extend extraction to support FLAC/WAV**

Replace the existing `extract_audio` function body with:

```rust
pub async fn extract_audio(
    app: tauri::AppHandle,
    input_path: String,
    output_path: String,
) -> Result<f64, String> {
    let extension = Path::new(&output_path)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("flac")
        .to_ascii_lowercase();

    let codec_args: Vec<&str> = match extension.as_str() {
        "wav" => vec!["-c:a", "pcm_s16le"],
        "mp3" => vec!["-acodec", "libmp3lame", "-ab", "128k"],
        _ => vec!["-c:a", "flac"],
    };

    let mut args = vec!["-i", &input_path, "-vn", "-ar", "16000", "-ac", "1"];
    args.extend(codec_args);
    args.extend(["-y", &output_path]);

    let output = app
        .shell()
        .sidecar("ffmpeg")
        .map_err(|e| e.to_string())?
        .args(args)
        .output()
        .await
        .map_err(|e| e.to_string())?;

    if output.status.code() != Some(0) {
        return Err(command_output_error("Falha ao extrair audio com ffmpeg", &output));
    }

    let duration = get_duration(&app, &output_path).await?;
    Ok(duration)
}
```

- [x] **Step 2: Add `detect_silences` command**

Add below `chunk_audio`:

```rust
#[command]
pub async fn detect_silences(
    app: tauri::AppHandle,
    input_path: String,
    noise_db: f64,
    min_duration_sec: f64,
) -> Result<Vec<SilenceRange>, String> {
    let filter = format!("silencedetect=noise={}dB:d={}", noise_db, min_duration_sec);
    let output = app
        .shell()
        .sidecar("ffmpeg")
        .map_err(|e| e.to_string())?
        .args(["-i", &input_path, "-af", &filter, "-f", "null", "-"])
        .output()
        .await
        .map_err(|e| e.to_string())?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    Ok(parse_silencedetect(&stderr))
}
```

- [x] **Step 3: Add `create_smart_chunks` command**

Add below `detect_silences`:

```rust
#[command]
pub async fn create_smart_chunks(
    app: tauri::AppHandle,
    input_path: String,
    output_dir: String,
    duration_sec: f64,
    options: Option<SmartChunkOptions>,
) -> Result<Vec<ExportedChunk>, String> {
    let opts = options.unwrap_or_default();
    fs::create_dir_all(&output_dir).map_err(|e| e.to_string())?;

    let silences = detect_silences(
        app.clone(),
        input_path.clone(),
        opts.silence_noise_db,
        opts.silence_min_duration_sec,
    )
    .await
    .unwrap_or_default();

    let plans = plan_smart_chunks(
        duration_sec,
        &silences,
        opts.target_sec,
        opts.min_sec,
        opts.max_sec,
        opts.overlap_sec,
    );

    let mut exported = Vec::new();
    for plan in plans {
        let chunk_duration = plan.end_sec - plan.start_sec;
        let audio_path = format!(
            "{}/chunk_{:03}.{}",
            output_dir.replace('\\', "/"),
            plan.index,
            opts.output_format
        );

        let output = app
            .shell()
            .sidecar("ffmpeg")
            .map_err(|e| e.to_string())?
            .args([
                "-ss",
                &format!("{:.3}", plan.start_sec),
                "-i",
                &input_path,
                "-t",
                &format!("{:.3}", chunk_duration),
                "-ar",
                "16000",
                "-ac",
                "1",
                "-c:a",
                if opts.output_format == "wav" { "pcm_s16le" } else { "flac" },
                "-y",
                &audio_path,
            ])
            .output()
            .await
            .map_err(|e| e.to_string())?;

        if output.status.code() != Some(0) {
            return Err(command_output_error("Falha ao exportar trecho com ffmpeg", &output));
        }

        exported.push(ExportedChunk {
            index: plan.index,
            audio_path,
            start_sec: plan.start_sec,
            end_sec: plan.end_sec,
            offset_sec: plan.offset_sec,
            duration_sec: chunk_duration,
        });
    }

    Ok(exported)
}
```

- [x] **Step 4: Register commands**

In `src-tauri/src/lib.rs`, add these entries to the existing `tauri::generate_handler!` command list after `commands::audio::chunk_audio`:

```rust
commands::audio::detect_silences,
commands::audio::create_smart_chunks,
```

- [x] **Step 5: Verify**

Run:

```powershell
cargo test commands::audio::tests --lib
cargo check
```

Expected: both commands exit `0`.

- [x] **Step 6: Commit**

If Git exists:

```powershell
git add src-tauri/src/commands/audio.rs src-tauri/src/lib.rs
git commit -m "feat: export silence-aware audio chunks"
```

## Task 4: Chunk Persistence

**Files:**

- Modify: `src-tauri/src/commands/db.rs`
- Modify: `src-tauri/src/lib.rs`

- [x] **Step 1: Add `processing_chunks` table**

Inside `init_db`, append this SQL before the closing quote of `execute_batch`:

```sql
CREATE TABLE IF NOT EXISTS processing_chunks (
    meeting_id TEXT NOT NULL,
    index_no INTEGER NOT NULL,
    audio_path TEXT NOT NULL,
    start_sec REAL NOT NULL,
    end_sec REAL NOT NULL,
    offset_sec REAL NOT NULL,
    duration_sec REAL NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    raw_segments_json TEXT,
    error_msg TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (meeting_id, index_no)
);
```

- [x] **Step 2: Add imports**

At the top of `db.rs`, add:

```rust
use crate::models::audio::{ExportedChunk, ProcessingChunkRecord};
```

- [x] **Step 3: Add chunk persistence commands**

Add after `update_meeting_status`:

```rust
#[command]
pub fn save_processing_chunks(
    state: tauri::State<'_, DbState>,
    meeting_id: String,
    chunks: Vec<ExportedChunk>,
) -> Result<(), String> {
    let db = state.0.lock().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();

    for chunk in chunks {
        db.execute(
            "INSERT OR IGNORE INTO processing_chunks
            (meeting_id, index_no, audio_path, start_sec, end_sec, offset_sec, duration_sec, status, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'pending', ?8, ?9)",
            params![
                meeting_id,
                chunk.index as i64,
                chunk.audio_path,
                chunk.start_sec,
                chunk.end_sec,
                chunk.offset_sec,
                chunk.duration_sec,
                now,
                now
            ],
        ).map_err(|e| e.to_string())?;
    }

    Ok(())
}

#[command]
pub fn get_processing_chunks(
    state: tauri::State<'_, DbState>,
    meeting_id: String,
) -> Result<Vec<ProcessingChunkRecord>, String> {
    let db = state.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = db
        .prepare(
            "SELECT meeting_id, index_no, audio_path, start_sec, end_sec, offset_sec, duration_sec, status, raw_segments_json, error_msg
             FROM processing_chunks
             WHERE meeting_id = ?1
             ORDER BY index_no ASC",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(params![meeting_id], |row| {
            Ok(ProcessingChunkRecord {
                meeting_id: row.get(0)?,
                index: row.get::<_, i64>(1)? as usize,
                audio_path: row.get(2)?,
                start_sec: row.get(3)?,
                end_sec: row.get(4)?,
                offset_sec: row.get(5)?,
                duration_sec: row.get(6)?,
                status: row.get(7)?,
                raw_segments_json: row.get(8)?,
                error_msg: row.get(9)?,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut chunks = Vec::new();
    for row in rows {
        chunks.push(row.map_err(|e| e.to_string())?);
    }
    Ok(chunks)
}

#[command]
pub fn update_processing_chunk_result(
    state: tauri::State<'_, DbState>,
    meeting_id: String,
    index: usize,
    status: String,
    raw_segments_json: Option<String>,
    error_msg: Option<String>,
) -> Result<(), String> {
    let db = state.0.lock().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();
    db.execute(
        "UPDATE processing_chunks
         SET status = ?1, raw_segments_json = ?2, error_msg = ?3, updated_at = ?4
         WHERE meeting_id = ?5 AND index_no = ?6",
        params![status, raw_segments_json, error_msg, now, meeting_id, index as i64],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
```

- [x] **Step 4: Register DB commands**

In `src-tauri/src/lib.rs`, add these entries to the existing `tauri::generate_handler!` command list after `commands::db::update_meeting_status`:

```rust
commands::db::save_processing_chunks,
commands::db::get_processing_chunks,
commands::db::update_processing_chunk_result,
```

- [x] **Step 5: Verify**

Run:

```powershell
cargo check
```

from `src-tauri`.

Expected: exit code `0`.

- [x] **Step 6: Commit**

If Git exists:

```powershell
git add src-tauri/src/commands/db.rs src-tauri/src/lib.rs
git commit -m "feat: persist processing chunks"
```

## Task 5: TypeScript Types and Tauri Wrappers

**Files:**

- Modify: `src/lib/types.ts`
- Modify: `src/lib/tauri.ts`

- [x] **Step 1: Add TypeScript audio types**

Append to `src/lib/types.ts`:

```ts
export interface SilenceRange {
  startSec: number;
  endSec: number;
}

export interface SmartChunkOptions {
  targetSec: number;
  minSec: number;
  maxSec: number;
  overlapSec: number;
  silenceMinDurationSec: number;
  silenceNoiseDb: number;
  outputFormat: 'flac' | 'wav';
}

export interface ExportedChunk {
  index: number;
  audioPath: string;
  startSec: number;
  endSec: number;
  offsetSec: number;
  durationSec: number;
}

export interface ProcessingChunkRecord extends ExportedChunk {
  meetingId: string;
  status: 'pending' | 'running' | 'done' | 'error';
  rawSegmentsJson: string | null;
  errorMsg: string | null;
}
```

- [x] **Step 2: Add wrappers**

Update the import in `src/lib/tauri.ts` to include new types:

```ts
import type {
  Meeting,
  TranscriptionSegment,
  DiarizedResult,
  SilenceRange,
  SmartChunkOptions,
  ExportedChunk,
  ProcessingChunkRecord,
} from './types';
```

Append these exports:

```ts
export const detectSilences = (inputPath: string, noiseDb = -35, minDurationSec = 0.45) =>
  invoke<SilenceRange[]>('detect_silences', { inputPath, noiseDb, minDurationSec });

export const createSmartChunks = (
  inputPath: string,
  outputDir: string,
  durationSec: number,
  options?: SmartChunkOptions
) =>
  invoke<ExportedChunk[]>('create_smart_chunks', { inputPath, outputDir, durationSec, options });

export const saveProcessingChunks = (meetingId: string, chunks: ExportedChunk[]) =>
  invoke<void>('save_processing_chunks', { meetingId, chunks });

export const getProcessingChunks = (meetingId: string) =>
  invoke<ProcessingChunkRecord[]>('get_processing_chunks', { meetingId });

export const updateProcessingChunkResult = (
  meetingId: string,
  index: number,
  status: ProcessingChunkRecord['status'],
  rawSegmentsJson?: string,
  errorMsg?: string
) =>
  invoke<void>('update_processing_chunk_result', {
    meetingId,
    index,
    status,
    rawSegmentsJson,
    errorMsg,
  });
```

- [x] **Step 3: Verify**

Run:

```powershell
npm run build
```

Expected: TypeScript and Vite build exit `0`.

- [x] **Step 4: Commit**

If Git exists:

```powershell
git add src/lib/types.ts src/lib/tauri.ts
git commit -m "feat: add smart chunk API wrappers"
```

## Task 6: Concurrent Transcription Queue

**Files:**

- Create: `src/lib/transcriptionQueue.ts`
- Create: `src/lib/transcriptionQueue.test.mjs`

- [x] **Step 1: Write failing queue test**

Create `src/lib/transcriptionQueue.test.mjs`:

```js
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { rmSync, mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";

const repo = process.cwd();
const outDir = join(process.env.TEMP, "meeting-minutes-transcription-queue-test");
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
      "src/lib/transcriptionQueue.ts",
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

const { transcribeChunksConcurrently } = require(join(outDir, "transcriptionQueue.js"));

const chunks = [
  { index: 0, audioPath: "a.flac", startSec: 0, endSec: 10, offsetSec: 0, durationSec: 10 },
  { index: 1, audioPath: "b.flac", startSec: 9, endSec: 20, offsetSec: 9, durationSec: 11 },
  { index: 2, audioPath: "c.flac", startSec: 19, endSec: 30, offsetSec: 19, durationSec: 11 },
];

let active = 0;
let maxActive = 0;
const progress = [];

const result = await transcribeChunksConcurrently({
  chunks,
  apiKey: "test-key",
  concurrency: 2,
  transcribeChunk: async (audioPath, apiKey, offsetSec) => {
    assert.equal(apiKey, "test-key");
    active += 1;
    maxActive = Math.max(maxActive, active);
    await new Promise((resolve) => setTimeout(resolve, audioPath === "a.flac" ? 30 : 5));
    active -= 1;
    return [{ id: 0, start: offsetSec, end: offsetSec + 1, text: audioPath }];
  },
  onChunkDone: (event) => progress.push(event),
});

assert.equal(maxActive, 2);
assert.deepEqual(result.map((segment) => segment.text), ["a.flac", "b.flac", "c.flac"]);
assert.deepEqual(result.map((segment) => segment.id), [0, 1, 2]);
assert.equal(progress.length, 3);
assert.equal(progress.at(-1).completedChunks, 3);
assert.equal(progress.at(-1).completedAudioSec, 32);
```

- [x] **Step 2: Run test to verify it fails**

Run:

```powershell
node src/lib/transcriptionQueue.test.mjs
```

Expected: fail because `src/lib/transcriptionQueue.ts` does not exist.

- [x] **Step 3: Implement queue**

Create `src/lib/transcriptionQueue.ts`:

```ts
import type { ExportedChunk, TranscriptionSegment } from './types';

export interface ChunkDoneEvent {
  chunkIndex: number;
  completedChunks: number;
  totalChunks: number;
  completedAudioSec: number;
  totalAudioSec: number;
}

export type ChunkTranscriber = (
  audioPath: string,
  apiKey: string,
  offsetSec: number
) => Promise<TranscriptionSegment[]>;

export interface TranscriptionQueueOptions {
  chunks: ExportedChunk[];
  apiKey: string;
  concurrency: number;
  transcribeChunk: ChunkTranscriber;
  onChunkDone?: (event: ChunkDoneEvent) => void;
}

export async function transcribeChunksConcurrently({
  chunks,
  apiKey,
  concurrency,
  transcribeChunk,
  onChunkDone,
}: TranscriptionQueueOptions): Promise<TranscriptionSegment[]> {
  const safeConcurrency = Math.max(1, Math.min(concurrency, chunks.length || 1));
  const results = new Map<number, TranscriptionSegment[]>();
  const totalAudioSec = chunks.reduce((sum, chunk) => sum + chunk.durationSec, 0);
  let completedChunks = 0;
  let completedAudioSec = 0;
  let nextIndex = 0;

  async function worker() {
    while (nextIndex < chunks.length) {
      const chunk = chunks[nextIndex];
      nextIndex += 1;

      const segments = await transcribeChunk(chunk.audioPath, apiKey, chunk.offsetSec);
      results.set(chunk.index, segments);
      completedChunks += 1;
      completedAudioSec += chunk.durationSec;
      onChunkDone?.({
        chunkIndex: chunk.index,
        completedChunks,
        totalChunks: chunks.length,
        completedAudioSec,
        totalAudioSec,
      });
    }
  }

  await Promise.all(Array.from({ length: safeConcurrency }, () => worker()));

  return chunks
    .flatMap((chunk) => results.get(chunk.index) ?? [])
    .sort((a, b) => a.start - b.start)
    .map((segment, id) => ({ ...segment, id }));
}
```

- [x] **Step 4: Run queue test**

Run:

```powershell
node src/lib/transcriptionQueue.test.mjs
```

Expected: exit code `0`.

- [x] **Step 5: Run build**

Run:

```powershell
npm run build
```

Expected: exit code `0`.

- [x] **Step 6: Commit**

If Git exists:

```powershell
git add src/lib/transcriptionQueue.ts src/lib/transcriptionQueue.test.mjs
git commit -m "feat: transcribe chunks concurrently"
```

## Task 7: Progress Math

**Files:**

- Create: `src/lib/pipelineProgress.ts`
- Create: `src/lib/pipelineProgress.test.mjs`
- Modify: `src/store/meetingStore.ts`

- [x] **Step 1: Write failing progress test**

Create `src/lib/pipelineProgress.test.mjs`:

```js
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { rmSync, mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";

const repo = process.cwd();
const outDir = join(process.env.TEMP, "meeting-minutes-pipeline-progress-test");
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
      "src/lib/pipelineProgress.ts",
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

const { derivePipelineProgress } = require(join(outDir, "pipelineProgress.js"));

assert.deepEqual(
  derivePipelineProgress({
    phase: "transcribe",
    completedAudioSec: 900,
    totalAudioSec: 1800,
    completedChunks: 3,
    totalChunks: 6,
    elapsedMs: 60_000,
  }),
  {
    percent: 50,
    title: "Transcrevendo em paralelo",
    detail: "3 de 6 trechos concluidos. 15m00s de 30m00s processados.",
    etaLabel: "1m00s restantes",
    speedLabel: "15.0x tempo real",
  }
);
```

- [x] **Step 2: Run test to verify it fails**

Run:

```powershell
node src/lib/pipelineProgress.test.mjs
```

Expected: fail because `src/lib/pipelineProgress.ts` does not exist.

- [x] **Step 3: Implement progress helper**

Create `src/lib/pipelineProgress.ts`:

```ts
export type PipelinePhase =
  | 'prepare_audio'
  | 'detect_speech'
  | 'create_chunks'
  | 'transcribe'
  | 'diarize'
  | 'generate'
  | 'complete';

export interface PipelineProgressInput {
  phase: PipelinePhase;
  completedAudioSec: number;
  totalAudioSec: number;
  completedChunks: number;
  totalChunks: number;
  elapsedMs: number;
}

export interface PipelineProgressView {
  percent: number;
  title: string;
  detail: string;
  etaLabel: string;
  speedLabel: string;
}

const PHASE_TITLES: Record<PipelinePhase, string> = {
  prepare_audio: 'Preparando audio',
  detect_speech: 'Detectando fala e pausas',
  create_chunks: 'Criando blocos inteligentes',
  transcribe: 'Transcrevendo em paralelo',
  diarize: 'Identificando falantes',
  generate: 'Montando ata final',
  complete: 'Processamento concluido',
};

function clamp(value: number, min: number, max: number) {
  return Math.min(Math.max(value, min), max);
}

function formatDuration(totalSec: number) {
  const safe = Math.max(0, Math.round(totalSec));
  const hours = Math.floor(safe / 3600);
  const minutes = Math.floor((safe % 3600) / 60);
  const seconds = safe % 60;
  if (hours > 0) return `${hours}h${String(minutes).padStart(2, '0')}m`;
  return `${minutes}m${String(seconds).padStart(2, '0')}s`;
}

export function derivePipelineProgress(input: PipelineProgressInput): PipelineProgressView {
  const totalAudioSec = Math.max(input.totalAudioSec, 0);
  const completedAudioSec = clamp(input.completedAudioSec, 0, totalAudioSec);
  const ratio = totalAudioSec > 0 ? completedAudioSec / totalAudioSec : 0;
  const percent = input.phase === 'complete' ? 100 : Math.round(ratio * 100);
  const elapsedSec = Math.max(input.elapsedMs / 1000, 0.001);
  const speed = completedAudioSec / elapsedSec;
  const remainingAudioSec = Math.max(totalAudioSec - completedAudioSec, 0);
  const remainingWallSec = speed > 0 ? remainingAudioSec / speed : 0;

  return {
    percent,
    title: PHASE_TITLES[input.phase],
    detail: `${input.completedChunks} de ${input.totalChunks} trechos concluidos. ${formatDuration(completedAudioSec)} de ${formatDuration(totalAudioSec)} processados.`,
    etaLabel: input.phase === 'complete' ? 'Concluido' : `${formatDuration(remainingWallSec)} restantes`,
    speedLabel: `${speed.toFixed(1)}x tempo real`,
  };
}
```

- [x] **Step 4: Extend store fields**

In `src/store/meetingStore.ts`, add fields to the interface:

```ts
progressEta: string;
progressSpeed: string;
setProgressStats: (progress: number, title: string, detail: string, eta: string, speed: string) => void;
```

Add initial state values:

```ts
progressEta: '',
progressSpeed: '',
```

Add setter:

```ts
setProgressStats: (progress, progressTitle, progressDetail, progressEta, progressSpeed) =>
  set({ progress, progressTitle, progressDetail, progressEta, progressSpeed }),
```

Add reset values:

```ts
progressEta: '',
progressSpeed: '',
```

- [x] **Step 5: Run tests and build**

Run:

```powershell
node src/lib/pipelineProgress.test.mjs
npm run build
```

Expected: both exit `0`.

- [x] **Step 6: Commit**

If Git exists:

```powershell
git add src/lib/pipelineProgress.ts src/lib/pipelineProgress.test.mjs src/store/meetingStore.ts
git commit -m "feat: derive pipeline progress stats"
```

## Task 8: Wire Processing Pipeline

**Files:**

- Modify: `src/pages/Processing.tsx`
- Modify: `src/lib/transcription.ts`

- [x] **Step 1: Update imports in `Processing.tsx`**

Add new Tauri wrappers:

```ts
  createSmartChunks,
  saveProcessingChunks,
  getProcessingChunks,
  updateProcessingChunkResult,
```

Add helpers:

```ts
import { transcribeChunksConcurrently } from "../lib/transcriptionQueue";
import { derivePipelineProgress, type PipelinePhase } from "../lib/pipelineProgress";
import type { ExportedChunk, ProcessingChunkRecord, TranscriptionSegment } from "../lib/types";
```

- [x] **Step 2: Add conversion helpers inside `Processing.tsx` above the component**

```ts
const toExportedChunk = (chunk: ProcessingChunkRecord): ExportedChunk => ({
  index: chunk.index,
  audioPath: chunk.audioPath,
  startSec: chunk.startSec,
  endSec: chunk.endSec,
  offsetSec: chunk.offsetSec,
  durationSec: chunk.durationSec,
});

const parseStoredSegments = (chunk: ProcessingChunkRecord): TranscriptionSegment[] => {
  if (!chunk.rawSegmentsJson) return [];
  try {
    return JSON.parse(chunk.rawSegmentsJson) as TranscriptionSegment[];
  } catch {
    return [];
  }
};
```

- [x] **Step 3: Replace old progress update helper**

Replace `updateProgress` with:

```ts
const startedAtRef = useRef<number>(Date.now());

const updatePipelineProgress = (
  phase: PipelinePhase,
  completedAudioSec: number,
  totalAudioSec: number,
  completedChunks: number,
  totalChunks: number,
) => {
  const view = derivePipelineProgress({
    phase,
    completedAudioSec,
    totalAudioSec,
    completedChunks,
    totalChunks,
    elapsedMs: Date.now() - startedAtRef.current,
  });
  setProgressStats(view.percent, view.title, view.detail, view.etaLabel, view.speedLabel);
};
```

Update the store destructuring to use `setProgressStats`, `progressEta`, and `progressSpeed`.

- [x] **Step 4: Build smart chunks with resume**

Replace the extraction and chunking section in `runPipeline` with:

```ts
startedAtRef.current = Date.now();
updatePipelineProgress("prepare_audio", 0, 0, 0, 0);

const sourceDir = getParentDir(meeting.filePath);
const audioOutput = joinPath(sourceDir, `${meetingId}_audio.flac`);
const durationSec = await extractAudio(meeting.filePath, audioOutput);

updatePipelineProgress("detect_speech", 0, durationSec, 0, 0);

let storedChunks = await getProcessingChunks(meetingId);
if (storedChunks.length === 0) {
  updatePipelineProgress("create_chunks", 0, durationSec, 0, 0);
  const chunkDir = joinPath(sourceDir, `${meetingId}_chunks`);
  const exportedChunks = await createSmartChunks(audioOutput, chunkDir, durationSec, {
    targetSec: 360,
    minSec: 180,
    maxSec: 480,
    overlapSec: 3,
    silenceMinDurationSec: 0.45,
    silenceNoiseDb: -35,
    outputFormat: "flac",
  });
  await saveProcessingChunks(meetingId, exportedChunks);
  storedChunks = await getProcessingChunks(meetingId);
}

const totalAudioSec = storedChunks.reduce((sum, chunk) => sum + chunk.durationSec, 0);
const completedStoredChunks = storedChunks.filter((chunk) => chunk.status === "done");
const completedSegments = completedStoredChunks.flatMap(parseStoredSegments);
const completedAudioSec = completedStoredChunks.reduce((sum, chunk) => sum + chunk.durationSec, 0);
const pendingChunks = storedChunks
  .filter((chunk) => chunk.status !== "done")
  .map(toExportedChunk);
```

- [x] **Step 5: Replace sequential transcription call**

Replace the existing call to `transcribeFile` with:

```ts
setStep("transcribe");
setStepStatus("transcribe", "running");
updatePipelineProgress(
  "transcribe",
  completedAudioSec,
  totalAudioSec,
  completedStoredChunks.length,
  storedChunks.length,
);

const newSegments = await transcribeChunksConcurrently({
  chunks: pendingChunks,
  apiKey: keys.groq,
  concurrency: 3,
  transcribeChunk: async (audioPath, apiKey, offsetSec) => {
    const chunk = pendingChunks.find((item) => item.audioPath === audioPath);
    if (chunk) {
      await updateProcessingChunkResult(meetingId, chunk.index, "running");
    }
    try {
      const segments = await transcribeChunk(audioPath, apiKey, offsetSec);
      if (chunk) {
        await updateProcessingChunkResult(
          meetingId,
          chunk.index,
          "done",
          JSON.stringify(segments),
        );
      }
      return segments;
    } catch (err) {
      if (chunk) {
        await updateProcessingChunkResult(
          meetingId,
          chunk.index,
          "error",
          undefined,
          formatError(err),
        );
      }
      throw err;
    }
  },
  onChunkDone: (event) => {
    updatePipelineProgress(
      "transcribe",
      completedAudioSec + event.completedAudioSec,
      totalAudioSec,
      completedStoredChunks.length + event.completedChunks,
      storedChunks.length,
    );
  },
});

const segments = [...completedSegments, ...newSegments]
  .sort((a, b) => a.start - b.start)
  .map((segment, segmentIndex) => ({ ...segment, id: segmentIndex }));
```

Remove the unused `transcribeFile` import after this replacement.

- [x] **Step 6: Render ETA and speed**

In the progress card, under `progressDetail`, add:

```tsx
<div className="mt-3 flex flex-wrap gap-2 text-xs text-gray-500">
  {progressEta && <span className="rounded bg-gray-100 px-2 py-1">{progressEta}</span>}
  {progressSpeed && <span className="rounded bg-gray-100 px-2 py-1">{progressSpeed}</span>}
</div>
```

- [x] **Step 7: Update later phase progress**

Before diarization:

```ts
updatePipelineProgress("diarize", totalAudioSec, totalAudioSec, storedChunks.length, storedChunks.length);
```

Before final generation:

```ts
updatePipelineProgress("generate", totalAudioSec, totalAudioSec, storedChunks.length, storedChunks.length);
```

Before navigating:

```ts
updatePipelineProgress("complete", totalAudioSec, totalAudioSec, storedChunks.length, storedChunks.length);
```

- [x] **Step 8: Run build**

Run:

```powershell
npm run build
```

Expected: exit code `0`.

- [x] **Step 9: Commit**

If Git exists:

```powershell
git add src/pages/Processing.tsx src/lib/transcription.ts
git commit -m "feat: wire smart chunk transcription pipeline"
```

## Task 9: Full Verification

**Files:**

- No source file changes expected.

- [x] **Step 1: Run TypeScript helper tests**

Run:

```powershell
node src/lib/transcriptionQueue.test.mjs
node src/lib/pipelineProgress.test.mjs
```

Expected: both exit `0`.

- [x] **Step 2: Run Rust tests**

Run:

```powershell
cargo test commands::audio::tests --lib
```

from `src-tauri`.

Expected: tests pass.

- [x] **Step 3: Run frontend build**

Run:

```powershell
npm run build
```

Expected: exit code `0`.

- [x] **Step 4: Run Rust check**

Run:

```powershell
cargo check
```

from `src-tauri`.

Expected: exit code `0`.

- [x] **Step 5: Run Tauri debug build**

Run:

```powershell
npm run tauri -- build --debug
```

Expected: exit code `0`, with debug bundles under `C:\tmp\cargo-target\debug\bundle`.

- [x] **Step 6: Manual smoke test with generated audio**

Generate a local fixture:

```powershell
$tmp = Join-Path $env:TEMP 'meeting-minutes-fast-core-fixture'
New-Item -ItemType Directory -Force -Path $tmp | Out-Null
$input = Join-Path $tmp 'fixture.wav'
& .\src-tauri\binaries\ffmpeg-x86_64-pc-windows-msvc.exe `
  -f lavfi -i "sine=frequency=900:duration=20" `
  -f lavfi -i "anullsrc=channel_layout=mono:sample_rate=16000:duration=2" `
  -f lavfi -i "sine=frequency=600:duration=20" `
  -filter_complex "[0:a][1:a][2:a]concat=n=3:v=0:a=1" `
  -ar 16000 -ac 1 -y $input
```

Open the app, select `fixture.wav`, process it, and verify:

- progress shows smart chunk/transcription phases;
- chunk count appears;
- ETA/speed labels appear;
- no duplicate processing starts in development mode;
- if processing is interrupted after one completed chunk, reopening the same meeting resumes from stored chunks.

- [x] **Step 7: Final summary**

Record:

- commands run;
- pass/fail status;
- any provider/API limitations observed;
- whether Git commit steps were skipped because the folder is not a Git repository.
