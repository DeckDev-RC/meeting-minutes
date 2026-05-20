# Adaptive Meeting Engine Design

## Goal

Reduce perceived end-to-end processing time without paid infrastructure by turning the current linear pipeline into an adaptive parallel pipeline.

## Design

The first implementation focuses on the highest-impact free path:

- Start local speaker-turn diarization as soon as the normalized WAV exists.
- Keep transcribing chunks in parallel while diarization runs.
- Extract meeting facts from completed transcript chunks without waiting for final speaker attribution.
- When transcription and speaker turns are both available, align turns to transcript text and generate the final minutes from compact facts plus the final speaker list.
- If the modern CPU backend is unavailable, fall back to the existing `auto` diarization path.
- Let the user optionally configure the expected number of speakers. When present, pass it into both speculative and fallback diarization to improve speaker-count stability.
- Run a bounded second pass on suspicious sub-windows only. The default fast path uses the modern CPU backend on one short extracted WAV window; Sherpa is not used as the default second pass because benchmark evidence showed it is too expensive for the speed target.

## Boundaries

This version does not package Python, does not require GPU, and does not make pyannote mandatory. It keeps the app free-first and uses the current Groq/Gemini API stages only where they already exist.

## Success Criteria

- Existing resumable chunk transcription continues to work.
- Diarization no longer waits for transcription before starting.
- Fact extraction no longer waits for diarization before starting.
- The final saved transcript still contains diarized speaker labels.
- The benchmark report still records the final speaker list and extracted facts.
