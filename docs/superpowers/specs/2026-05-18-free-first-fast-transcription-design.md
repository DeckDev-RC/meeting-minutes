# Free-First Fast Transcription Pipeline Design

Date: 2026-05-18
Project: Meeting Minutes AI
Decision: Option B, free-first

## Goal

Build a fast and accurate meeting-processing pipeline for long recordings, including videos and MP3 files up to roughly 3 hours, without adding paid defaults.

The "wow" target is:

- process long meetings much faster than real time;
- keep transcription quality high enough for useful minutes;
- avoid making users feel blind during processing;
- recover from interruptions and API/rate-limit failures;
- use free or local techniques wherever practical.

## Non-Goals

- No paid-only dependency in the default path.
- No GPU requirement for the MVP.
- No full local ASR bundle in the first implementation, because shipping Whisper models would add large downloads and support complexity.
- No complex speaker voiceprint database in the MVP.

## Current Pipeline

The current app does:

1. Extract audio to MP3, 16 kHz, mono.
2. Split audio into fixed 20-minute chunks.
3. Send chunks sequentially to Groq `whisper-large-v3-turbo`.
4. Ask Gemini to infer speaker labels from text.
5. Ask Gemini to generate one final HTML meeting minutes document.

The main problems are:

- fixed chunks can cut sentences and reduce punctuation quality;
- sequential API calls waste time on long recordings;
- MP3 is lossy and less ideal for ASR than FLAC/WAV;
- text-only diarization guesses speakers without voice evidence;
- no quality gate catches suspicious transcript sections;
- long final prompts can become fragile and expensive in tokens.

## Free-First Strategy

Default processing should use:

- bundled FFmpeg for extraction, silence detection, duration, and chunk export;
- local chunk planning and merge logic;
- existing Groq transcription key if available;
- existing Gemini key only for text synthesis and correction;
- no paid diarization provider by default;
- no automatic retry strategy that burns API quota indefinitely.

Optional future modes may include:

- local faster-whisper for users who accept model downloads;
- local or Hugging Face pyannote for audio-based diarization;
- OpenAI diarized transcription as a paid/optional accuracy mode.

The app should clearly label any optional provider that can incur cost.

Cost guard:

- default mode must not require a paid provider;
- external APIs may be used only with user-provided keys;
- the UI should describe providers as "uses your configured API quota", not "free forever";
- retries must be bounded to avoid accidental quota burn;
- if a provider returns quota/rate/billing errors, the app should pause and keep partial results instead of looping;
- optional paid accuracy modes must require explicit user opt-in.

## Proposed Pipeline

### 1. Audio Prepare

Convert source media into an ASR-friendly intermediate file.

Default:

- `16 kHz`;
- mono;
- FLAC for long files because it is lossless and smaller;
- WAV option for lower latency if file size is acceptable.

Rationale:

- Groq documentation says their STT models downsample to 16 kHz mono and recommends client-side conversion for large files.
- FLAC avoids MP3 generation loss while keeping upload size manageable.

### 2. Speech-Aware Chunk Planner

Replace fixed 20-minute chunks with speech-aware chunks.

Algorithm:

1. Run FFmpeg `silencedetect` over the extracted audio.
2. Build a silence map.
3. Create target chunks around 4-8 minutes.
4. Prefer cut points inside silence.
5. Add 2-5 seconds of overlap.
6. Keep absolute `startSec` and `endSec` for every chunk.

Fallback:

- If silence detection fails or the audio has no usable silence, split by time with overlap.

Why FFmpeg first:

- It is already bundled.
- It is free and local.
- It avoids adding Python/PyTorch to the MVP.

Possible later upgrade:

- Silero VAD via ONNX for better speech detection in noisy files.

### 3. Concurrent Transcription Queue

Process chunks concurrently instead of sequentially.

Default:

- start with concurrency `3`;
- allow configuration up to `6`;
- reduce concurrency automatically on `429` rate-limit responses;
- retry transient errors with bounded exponential backoff;
- stop retrying after a small limit and preserve partial results.

Each request uses:

- `response_format=verbose_json`;
- `timestamp_granularities[]=segment`;
- language `pt`;
- model `whisper-large-v3-turbo` by default.

Optional high-accuracy retry:

- only reprocess suspicious chunks with `whisper-large-v3`.
- Do not reprocess the whole meeting unless the user chooses an accuracy mode.

### 4. Transcript Merge

Merge chunk outputs using timestamp offsets and overlap handling.

Rules:

- shift local timestamps by chunk `startSec`;
- drop duplicate overlap segments based on time intersection and text similarity;
- preserve raw chunk metadata for debugging;
- sort final segments by absolute start time.

This prevents repeated text at chunk boundaries while keeping context.

### 5. Quality Gate

Score every segment/chunk using metadata from `verbose_json`.

Signals:

- `avg_logprob`;
- `compression_ratio`;
- `no_speech_prob`;
- empty text on speech-heavy areas;
- unusually repetitive text;
- segment duration too long with little content.

Output:

- confidence status: `ok`, `review`, or `retry`;
- list of suspicious time ranges;
- explanation in user-facing language.

Default action:

- retry only `retry` chunks once.
- mark `review` chunks in the transcript but continue the pipeline.

### 6. Refinement Pass

Use the LLM only where it adds value.

Default:

- clean punctuation and paragraph breaks after ASR;
- never invent missing content;
- keep timestamps and speaker labels stable;
- process in windows, not one giant prompt.

Inputs:

- transcript segments;
- quality flags;
- optional user glossary: company names, participant names, acronyms, product names.

This helps Portuguese meeting text without spending tokens on unnecessary rework.

### 7. Free-First Diarization

MVP default:

- keep current Gemini text-based speaker inference, but make it more constrained.
- ask the user optionally for number of speakers or participant names before processing.
- use turn-taking cues and timestamps, but label uncertain speaker changes as uncertain.

Why not pyannote by default:

- pyannote is powerful and can run locally, but it adds Python/PyTorch/model setup and Hugging Face model acceptance.
- It should be an optional "better speaker detection" module after the faster core pipeline is reliable.

Future optional free/local mode:

- pyannote community model for users who accept setup/model download.
- Align pyannote speaker turns with Whisper segments by timestamp overlap.

### 8. Minutes Map-Reduce

Do not send a 3-hour transcript as one huge prompt.

Steps:

1. Split transcript into topic/time windows.
2. For each window, extract:
   - summary;
   - decisions;
   - action items;
   - dates/deadlines;
   - blockers;
   - open questions.
3. Merge extracted structures.
4. Deduplicate actions and decisions.
5. Generate final HTML.

This is faster, more stable, and easier to recover if one LLM call fails.

## User Experience

The processing screen should show:

- percent complete based on audio seconds processed;
- current phase;
- number of chunks done / total;
- ETA;
- current speed factor, for example `32x tempo real`;
- warnings for rate limits or low-confidence sections;
- partial transcript preview as chunks finish;
- final quality summary.

Suggested phases:

1. Preparando audio
2. Detectando fala e pausas
3. Criando blocos inteligentes
4. Transcrevendo em paralelo
5. Revisando trechos suspeitos
6. Identificando falantes
7. Extraindo decisoes e acoes
8. Montando ata final

User controls:

- cancel processing;
- continue in background;
- retry failed chunks;
- export partial transcript if final generation fails.

## Persistence and Resume

Long processing needs durable jobs.

Store:

- job id;
- meeting id;
- audio path;
- chunk plan;
- chunk status;
- raw ASR response per chunk;
- merged transcript;
- quality flags;
- summaries per window;
- final minutes HTML.

If the app closes, it should resume from the last completed chunk instead of restarting.

## Configuration

Initial settings:

- mode: `Rapido balanceado` default;
- max concurrency: default `3`;
- chunk target minutes: default `6`;
- overlap seconds: default `3`;
- retry suspicious chunks: enabled;
- glossary: optional text area;
- known speakers: optional names.

Advanced settings should stay hidden unless the user expands them.

## Error Handling

Errors should be recoverable and concrete.

Examples:

- API key missing: ask user to configure key.
- Rate limit: lower concurrency and wait.
- Chunk failed: retry that chunk only.
- LLM final generation failed: keep transcript and per-block summaries.
- Low confidence: finish the ata but show warning and time ranges.

## Testing Strategy

Use a small fixture suite:

- 30-second clean audio;
- 5-minute audio with silence;
- synthetic 15-minute long fixture generated with FFmpeg;
- video input with one audio track;
- failure fixture for API retry logic.

Unit tests:

- silence map to chunk plan;
- chunk offset math;
- overlap deduplication;
- quality scoring;
- progress and ETA calculation.

Integration tests:

- run FFmpeg extraction and chunking locally;
- mock Groq/Gemini responses;
- resume interrupted job;
- generate final minutes from stored intermediate data.

Manual QA:

- 30 min, 1h, and 3h recordings;
- compare runtime, confidence warnings, and final minutes quality.

## Rollout Plan

Phase 1: Fast core

- FLAC/WAV extraction.
- FFmpeg silence-aware chunking.
- concurrent Groq queue.
- progress by audio seconds.
- resume state for chunks.

Phase 2: Quality core

- merge with overlap dedupe.
- metadata quality scoring.
- selective retry.
- glossary/prompt support.

Phase 3: Better minutes

- map-reduce extraction.
- final structured merge.
- quality summary in UI.

Phase 4: Optional speaker upgrade

- pyannote local module or other free/local diarization path.
- timestamp alignment with transcript.

## Key Risks

- Provider free tiers and rate limits can change.
- Parallel requests can hit rate limits if concurrency is too high.
- Silence detection is weaker than neural VAD in noisy meetings.
- Text-based diarization remains approximate.
- Local diarization/ASR can add heavy installation requirements.

## Acceptance Criteria

- A 3-hour recording does not require manual chunking.
- The app can resume after interruption without restarting from zero.
- Processing runs multiple transcription chunks concurrently.
- Progress and ETA are understandable to a non-technical user.
- Suspicious transcript sections are flagged instead of silently trusted.
- Final minutes are generated from structured intermediate summaries.
- No paid-only provider is required in the default path.

## Sources

- Groq Speech to Text docs: https://console.groq.com/docs/speech-to-text
- Whisper paper: https://arxiv.org/abs/2212.04356
- Whisper large-v3-turbo model card: https://huggingface.co/openai/whisper-large-v3-turbo
- WhisperX paper: https://arxiv.org/abs/2303.00747
- WhisperX repository: https://github.com/m-bain/whisperX
- faster-whisper repository: https://github.com/SYSTRAN/faster-whisper
- CTranslate2 repository: https://github.com/OpenNMT/CTranslate2
- pyannote.audio repository: https://github.com/pyannote/pyannote-audio
- Silero VAD repository: https://github.com/snakers4/silero-vad
- FFmpeg filters documentation: https://ffmpeg.org/ffmpeg-filters.html
- OpenAI Speech to Text docs: https://developers.openai.com/api/docs/guides/speech-to-text
- NN/g usability heuristics: https://www.nngroup.com/articles/ten-usability-heuristics/
