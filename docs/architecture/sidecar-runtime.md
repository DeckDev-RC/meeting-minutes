# Rejected Sidecar Runtime Investigation

## Decision

The installed app should remain a single installer for regular users. The user should not install Python, create virtual environments, or download model assets manually.

The runtime path stays as the current bundled diarization runtime. PyInstaller CLI sidecar and persistent sidecar were benchmarked and rejected as runtime replacements on 2026-05-24. They remain only as investigation artifacts.

## Current Runtime

`meeting-minutes` currently bundles the diarization runtime as a Tauri resource:

- `src-tauri/resources/diarize/.venv-diarize`
- `src-tauri/resources/diarize/scripts/diarize_cpu_backend.py`
- `src-tauri/tauri.conf.json` maps that directory as the `diarize` resource

This is heavier than a small sidecar executable, but it has one important advantage: it already works in the installed app and keeps model/runtime behavior identical to the benchmarked local setup.

## Investigation Result

Eskuta uses a PyInstaller sidecar, but benchmark data did not justify copying that runtime shape for `meeting-minutes`.

Measured outcome:

- PyInstaller `onefile` adds extraction/startup cost on every call.
- PyInstaller `onedir` was functional but slower in measured diarization/transcription samples.
- Persistent source sidecar reused models correctly, but did not beat the current backend on the 31 chunk real meeting.
- Current bundled runtime remains faster and simpler operationally for the app.

Therefore sidecar is not a Phase 4 target and must not become the default runtime.

## Archived Experimental Contract

The experimental entrypoint used during the investigation was:

```powershell
python scripts\meeting_minutes_sidecar.py health
python scripts\meeting_minutes_sidecar.py serve
python scripts\meeting_minutes_sidecar.py transcribe-local --input request.json --output response.json
python scripts\meeting_minutes_sidecar.py diarize-modern-cpu --input request.json --output response.json
```

Build command:

```powershell
npm run sidecar:build
```

The sidecar is not the default runtime and is no longer a benchmark candidate for Phase 4. Keep this contract only for reproducing historical benchmark results.

`diarize-modern-cpu` response includes the SDD turn contract:

```json
{
  "ok": true,
  "command": "diarize-modern-cpu",
  "turns": [{ "start": 0.0, "end": 4.2, "speakerIndex": 0 }],
  "telemetry": {
    "backend": "sidecar-modern-cpu",
    "wallClockSec": 42.1,
    "commandWallClockSec": 42.4
  }
}
```

`transcribe-local` response keeps the faster-whisper report and adds normalized sidecar telemetry:

```json
{
  "ok": true,
  "command": "transcribe-local",
  "segments": [],
  "telemetry": {
    "backend": "sidecar-faster-whisper",
    "model": "turbo",
    "speedX": 12.0
  }
}
```

## Persistent Sidecar Outcome

The first PyInstaller benchmark showed that one process per command is the wrong shape for heavy local ML runtimes:

- `onefile` adds extraction/startup cost on every call;
- `onedir` avoids extraction but still showed runtime regressions in the measured samples;
- the source Python wrapper is already near parity with the current backend.

The experimental `serve` command was tested:

```powershell
python scripts\meeting_minutes_sidecar.py serve
```

It speaks newline-delimited JSON on stdin/stdout:

```json
{"id":"1","command":"health"}
{"id":"2","command":"transcribe-local","request":{"audioPath":"sample.flac","outputDir":"out"}}
{"id":"3","command":"shutdown"}
```

Each response is one JSON line and carries the same `id`.

Implemented persistence scope:

- faster-whisper engine is cached while model/device/compute settings remain the same;
- diarization function is cached for single-worker requests;
- parallel diarization uses a persistent worker pool keyed by the normalized worker count. Each worker loads its own diarization function once and reuses it across requests.

If a later request changes the parallel worker count, the old pool is shut down and rebuilt for the new size. This avoids sharing one model instance across threads and keeps the diarization algorithm/output path equivalent to the current backend.

This is not wired into Tauri and should not be wired as a runtime replacement. The real 31 chunk benchmark showed `312.74s` warm persistent sidecar versus `305.49s` current backend.

## Boundary Modules Added

The Rust modules below define the seams for incremental migration without a rewrite:

- `commands/audio_chunker.rs`
- `commands/transcription_router.rs`
- `commands/minutes_pipeline.rs`
- `commands/minutes_validator.rs`

They keep the current commands working while giving us smaller units to move logic into over time.
