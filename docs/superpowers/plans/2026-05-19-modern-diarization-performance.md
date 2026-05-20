# Modern Diarization Performance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reduce the measured diarization bottleneck with selectable `fast`, `hybrid`, and `precise` modes, then benchmark the modes on the existing AMI run without extra API calls.

**Architecture:** Keep the current free/local `sherpa-onnx` stack. Add a small mode layer around diarization: `fast` uses transcript heuristics, `precise` uses full-audio sherpa, and `hybrid` runs sherpa per chunk in parallel and stitches speakers through overlap windows. The app will use `auto`, selecting `hybrid` when chunk metadata exists and falling back safely.

**Tech Stack:** Rust/Tauri 2, `sherpa-onnx`, existing smart audio chunks, existing benchmark artifacts, React/TypeScript wrapper updates.

---

### Task 1: Add Diarization Mode Types And Selection

**Files:**
- Modify: `src-tauri/src/commands/diarize.rs`

- [x] **Step 1: Write failing tests**

Add tests proving mode parsing and thread selection:

```rust
#[test]
fn diarization_mode_parses_supported_values() {
    assert_eq!(DiarizationMode::from_option(Some("fast".to_string())), DiarizationMode::Fast);
    assert_eq!(DiarizationMode::from_option(Some("hybrid".to_string())), DiarizationMode::Hybrid);
    assert_eq!(DiarizationMode::from_option(Some("precise".to_string())), DiarizationMode::Precise);
    assert_eq!(DiarizationMode::from_option(Some("auto".to_string())), DiarizationMode::Auto);
    assert_eq!(DiarizationMode::from_option(Some("bad".to_string())), DiarizationMode::Auto);
    assert_eq!(DiarizationMode::from_option(None), DiarizationMode::Auto);
}

#[test]
fn diarization_thread_count_is_bounded() {
    assert_eq!(normalize_diarization_threads(Some(0), 20), 2);
    assert_eq!(normalize_diarization_threads(Some(12), 20), 12);
    assert_eq!(normalize_diarization_threads(Some(64), 20), 16);
    assert_eq!(normalize_diarization_threads(None, 20), 8);
}
```

- [x] **Step 2: Run tests and verify RED**

Run:

```bash
cargo test --lib commands::diarize::tests::diarization_mode_parses_supported_values commands::diarize::tests::diarization_thread_count_is_bounded -- --nocapture
```

Expected: compile failure because `DiarizationMode::from_option` and `normalize_diarization_threads` do not exist.

- [x] **Step 3: Implement minimal mode/thread code**

Add `DiarizationMode`, parsing, and bounded thread normalization. Default should use up to `8` threads, never less than `2` for neural diarization, and cap at `16`.

- [x] **Step 4: Run full Rust lib tests**

Run:

```bash
cargo test --lib -- --nocapture
```

Expected: all tests pass.

### Task 2: Add Hybrid Chunk Diarization

**Files:**
- Modify: `src-tauri/src/commands/diarize.rs`
- Use existing type: `src-tauri/src/models/audio.rs::ExportedChunk`

- [x] **Step 1: Write failing stitching tests**

Add tests proving chunk-local results can be shifted and stitched:

```rust
#[test]
fn shifted_segments_for_chunk_use_chunk_relative_time() {
    let chunk = ExportedChunk {
        index: 1,
        audio_path: "chunk.flac".to_string(),
        start_sec: 100.0,
        end_sec: 200.0,
        offset_sec: 100.0,
        duration_sec: 100.0,
    };
    let segments = vec![
        TranscriptionSegment { id: 1, start: 110.0, end: 115.0, text: "hello".to_string() },
        TranscriptionSegment { id: 2, start: 220.0, end: 225.0, text: "outside".to_string() },
    ];

    let shifted = segments_for_chunk(&segments, &chunk);

    assert_eq!(shifted.len(), 1);
    assert_eq!(shifted[0].start, 10.0);
    assert_eq!(shifted[0].end, 15.0);
}

#[test]
fn stitch_chunk_results_maps_overlap_speakers() {
    let first = DiarizedResult {
        speakers: vec!["Falante 1".to_string()],
        segments: vec![DiarizedSegment {
            speaker: "Falante 1".to_string(),
            start: 0.0,
            end: 10.0,
            text: "same overlap".to_string(),
        }],
    };
    let second = DiarizedResult {
        speakers: vec!["Falante 1".to_string()],
        segments: vec![DiarizedSegment {
            speaker: "Falante 1".to_string(),
            start: 8.0,
            end: 18.0,
            text: "same overlap continues".to_string(),
        }],
    };

    let stitched = stitch_diarized_chunk_results(vec![first, second], 3.0);

    assert_eq!(stitched.speakers, vec!["Falante 1"]);
    assert_eq!(stitched.segments.len(), 1);
    assert_eq!(stitched.segments[0].speaker, "Falante 1");
    assert_eq!(stitched.segments[0].start, 0.0);
    assert_eq!(stitched.segments[0].end, 18.0);
}
```

- [x] **Step 2: Run tests and verify RED**

Run:

```bash
cargo test --lib commands::diarize::tests::shifted_segments_for_chunk_use_chunk_relative_time commands::diarize::tests::stitch_chunk_results_maps_overlap_speakers -- --nocapture
```

Expected: compile failure because helper functions do not exist.

- [x] **Step 3: Implement chunk helper functions**

Implement:
- `segments_for_chunk(segments, chunk)` to select overlapping transcript segments and shift times by `chunk.offset_sec`.
- `shift_diarized_result(result, offset)` to restore absolute times.
- `stitch_diarized_chunk_results(results, overlap_sec)` to merge adjacent same-speaker overlap and preserve a compact speaker list.

- [x] **Step 4: Add hybrid runner**

Implement `diarize_audio_by_chunks_with_sherpa(audio_chunks, segments, assets, expected_speakers, num_threads)` using `tokio::task::spawn_blocking` per chunk with bounded concurrency. Each chunk uses `diarize_audio_with_sherpa` on chunk audio, then shifts results back and stitches them.

- [x] **Step 5: Run full Rust lib tests**

Run:

```bash
cargo test --lib -- --nocapture
```

Expected: all tests pass.

### Task 3: Wire Modes Into Tauri And Frontend

**Files:**
- Modify: `src-tauri/src/commands/diarize.rs`
- Modify: `src/lib/tauri.ts`
- Modify: `src/pages/Processing.tsx`

- [x] **Step 1: Update command signature**

Change `diarize_transcription_end_to_end` to accept:

```rust
audio_chunks: Option<Vec<ExportedChunk>>,
mode: Option<String>,
num_threads: Option<i32>,
expected_speakers: Option<i32>,
```

Use selection:
- `fast`: local heuristic only.
- `precise`: full-audio sherpa.
- `hybrid`: chunk sherpa when chunks exist, otherwise precise.
- `auto`: hybrid when chunks exist, precise otherwise.

- [x] **Step 2: Update TS wrapper and processing call**

In `src/lib/tauri.ts`, pass optional `audioChunks`, `mode`, `numThreads`, and `expectedSpeakers`.

In `Processing.tsx`, call:

```ts
const diarized = await diarizeTranscriptionEndToEnd(audioOutput, segmentsJson, {
  audioChunks: storedChunks.map(toExportedChunk),
  mode: "auto",
});
```

- [x] **Step 3: Run frontend build**

Run:

```bash
npm run build
```

Expected: TypeScript and Vite build pass.

### Task 4: Extend Benchmark Runner And Run Diarization-Only Benchmarks

**Files:**
- Modify: `src-tauri/src/bin/e2e_benchmark.rs`
- Create: `src-tauri/src/bin/diarization_benchmark.rs`

- [x] **Step 1: Add mode flags to E2E runner**

Add CLI flags:
- `--diarization-mode auto|fast|hybrid|precise`
- `--diarization-threads N`

The runner should include `diarizationMode` and `diarizationThreads` in the JSON report.

- [x] **Step 2: Create diarization-only runner**

Create a runner that loads:
- `--audio benchmarks/runs/ami-es2002a-e2e/normalized-audio.wav`
- `--chunks benchmarks/runs/ami-es2002a-e2e/chunks.json`
- `--segments benchmarks/runs/ami-es2002a-e2e/transcription-segments.json`
- `--mode fast|hybrid|precise`
- `--threads N`
- `--out-dir benchmarks/runs/<id>`

It should write `diarization-benchmark-report.json` with duration, mode, thread count, speaker count, segment count, and output file path.

- [x] **Step 3: Run real no-API benchmark matrix**

Run:

```bash
cargo run --bin diarization_benchmark -- --mode fast --threads 8 --audio ..\benchmarks\runs\ami-es2002a-e2e\normalized-audio.wav --chunks ..\benchmarks\runs\ami-es2002a-e2e\chunks.json --segments ..\benchmarks\runs\ami-es2002a-e2e\transcription-segments.json --out-dir ..\benchmarks\runs\diarization-fast
cargo run --bin diarization_benchmark -- --mode hybrid --threads 4 --audio ..\benchmarks\runs\ami-es2002a-e2e\normalized-audio.wav --chunks ..\benchmarks\runs\ami-es2002a-e2e\chunks.json --segments ..\benchmarks\runs\ami-es2002a-e2e\transcription-segments.json --out-dir ..\benchmarks\runs\diarization-hybrid-t4
cargo run --bin diarization_benchmark -- --mode precise --threads 8 --audio ..\benchmarks\runs\ami-es2002a-e2e\normalized-audio.wav --chunks ..\benchmarks\runs\ami-es2002a-e2e\chunks.json --segments ..\benchmarks\runs\ami-es2002a-e2e\transcription-segments.json --out-dir ..\benchmarks\runs\diarization-precise-t8
```

Expected: all produce JSON reports without using Groq or Gemini.

### Task 5: Final Verification

**Files:**
- Verify all touched files.

- [x] **Step 1: Rust tests**

Run:

```bash
cargo test --lib -- --nocapture
```

Expected: all tests pass.

- [x] **Step 2: Rust binaries compile**

Run:

```bash
cargo check --bin meeting-minutes
cargo check --bin e2e_benchmark
cargo check --bin diarization_benchmark
```

Expected: all compile.

- [x] **Step 3: Frontend build**

Run:

```bash
npm run build
```

Expected: TypeScript and Vite build pass.

- [x] **Step 4: Report real benchmark facts**

Summarize:
- baseline full diarization: `324.11s`
- new fast time
- new hybrid time
- new precise tuned time
- estimated 3h projection for each mode
- accuracy caveat: no DER/factual precision claim until manual gabarito exists.

### Task 6: Integrate Free Modern CPU Backend

**Files:**
- Modify: `src-tauri/src/commands/diarize.rs`
- Modify: `src-tauri/src/bin/diarization_benchmark.rs`
- Modify: `src-tauri/src/bin/e2e_benchmark.rs`
- Modify: `src/lib/types.ts`
- Modify: `docs/evaluation/free-benchmark-protocol.md`

- [x] **Step 1: Add `modern-cpu` mode**

Add `DiarizationMode::ModernCpu`, parse aliases `modern-cpu`, `diarize-cpu`, and `cpu`, and expose the mode in TypeScript.

- [x] **Step 2: Call the Python CPU backend from Rust**

Resolve `.venv-diarize/Scripts/python.exe` and `scripts/diarize_cpu_backend.py` from the project tree, run the backend in a blocking worker, read `diarized-transcription.json`, and align the detected speaker turns to the existing transcript text.

- [x] **Step 3: Preserve fallback behavior**

Use `modern-cpu` first in app `auto` mode when the backend exists, then fall back to Sherpa hybrid/precise/local if it is not installed. For explicit `modern-cpu`, return the backend error instead of silently benchmarking another algorithm.

- [x] **Step 4: Avoid unnecessary Sherpa setup for explicit `modern-cpu` benchmarks**

The benchmark runners pass asset paths without downloading/checking Sherpa models when the requested mode is `modern-cpu`.

- [x] **Step 5: Run real integrated benchmark**

Command:

```powershell
cargo run --bin diarization_benchmark -- --mode modern-cpu --expected-speakers 4 --audio ..\benchmarks\runs\ami-es2002a-e2e\normalized-audio.wav --chunks ..\benchmarks\runs\ami-es2002a-e2e-wav-chunks\chunks.json --segments ..\benchmarks\runs\ami-es2002a-e2e\transcription-segments.json --out-dir ..\benchmarks\runs\diarization-modern-cpu-num4-rust
```

Result: `103.50s` wall clock, RTF `0.081`, `12.30x`, projected 3h diarization `14.6 min`, 4 speakers.
