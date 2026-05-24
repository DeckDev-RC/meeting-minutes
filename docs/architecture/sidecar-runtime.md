# Sidecar Runtime Strategy

## Decision

The installed app should remain a single installer for regular users. The user should not install Python, create virtual environments, or download model assets manually.

## Current Runtime

`meeting-minutes` currently bundles the diarization runtime as a Tauri resource:

- `src-tauri/resources/diarize/.venv-diarize`
- `src-tauri/resources/diarize/scripts/diarize_cpu_backend.py`
- `src-tauri/tauri.conf.json` maps that directory as the `diarize` resource

This is heavier than a small sidecar executable, but it has one important advantage: it already works in the installed app and keeps model/runtime behavior identical to the benchmarked local setup.

## Eskuta-Inspired Target

Eskuta uses a PyInstaller sidecar. That is the right long-term packaging shape for us too, but only after benchmark parity:

1. Build a sidecar executable that exposes the same diarization and local ASR commands used today.
2. Run the current 3h and 4h46 benchmark meetings against bundled-venv and sidecar modes.
3. Compare:
   - total wall time;
   - diarization stage time;
   - speaker count stability;
   - action/decision/evidence counts;
   - installer size;
   - first-run startup time.
4. Switch the default only if quality is equivalent and startup/runtime cost does not regress.

## Why Not Switch Immediately

PyInstaller can change startup time, import resolution, model file lookup, and native library loading. For this app, runtime behavior is more important than bundle elegance. The current bundled runtime stays until a sidecar build proves parity.

## Experimental Sidecar Contract

The Phase 4 experimental entrypoint is:

```powershell
python scripts\meeting_minutes_sidecar.py health
python scripts\meeting_minutes_sidecar.py transcribe-local --input request.json --output response.json
python scripts\meeting_minutes_sidecar.py diarize-modern-cpu --input request.json --output response.json
```

Build command:

```powershell
npm run sidecar:build
```

The sidecar is not the default runtime. It is only a benchmark candidate until it beats or matches the bundled runtime matrix.

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

## Boundary Modules Added

The Rust modules below define the seams for incremental migration without a rewrite:

- `commands/audio_chunker.rs`
- `commands/transcription_router.rs`
- `commands/minutes_pipeline.rs`
- `commands/minutes_validator.rs`

They keep the current commands working while giving us smaller units to move logic into over time.
