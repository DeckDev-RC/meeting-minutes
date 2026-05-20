import argparse
import json
import os
import time
import wave
from pathlib import Path


MODEL_ID = "pyannote/speaker-diarization-community-1"


def normalize_speaker_name(value):
    raw = str(value or "").strip()
    if raw.upper().startswith("SPEAKER_"):
        suffix = raw.split("_", 1)[1].lstrip("0") or "0"
        if suffix.isdigit():
            return f"Falante {int(suffix) + 1}"
    if raw.lower().startswith("speaker_"):
        suffix = raw.split("_", 1)[1].lstrip("0") or "0"
        if suffix.isdigit():
            return f"Falante {int(suffix) + 1}"
    return raw or "Falante 1"


def audio_duration_sec(audio_path):
    try:
        with wave.open(str(audio_path), "rb") as handle:
            frames = handle.getnframes()
            rate = handle.getframerate()
            return frames / float(rate) if rate else 0.0
    except Exception:
        return 0.0


def iter_annotation_segments(annotation):
    if annotation is None:
        return

    if hasattr(annotation, "itertracks"):
        for turn, _, speaker in annotation.itertracks(yield_label=True):
            yield turn, speaker
        return

    for item in annotation:
        if len(item) == 2:
            turn, speaker = item
        elif len(item) == 3:
            turn, _, speaker = item
        else:
            continue
        yield turn, speaker


def output_annotation(output):
    return getattr(output, "exclusive_speaker_diarization", None) or getattr(
        output, "speaker_diarization", None
    )


def read_pcm_wav(audio_path):
    try:
        import numpy as np
    except Exception as exc:
        raise RuntimeError("numpy is required by the pyannote backend") from exc

    with wave.open(str(audio_path), "rb") as handle:
        channels = handle.getnchannels()
        sample_width = handle.getsampwidth()
        sample_rate = handle.getframerate()
        frames = handle.readframes(handle.getnframes())

    if channels <= 0:
        raise RuntimeError(f"Invalid WAV channel count in {audio_path}")

    if sample_width == 1:
        samples = (np.frombuffer(frames, dtype=np.uint8).astype(np.float32) - 128.0) / 128.0
    elif sample_width == 2:
        samples = np.frombuffer(frames, dtype="<i2").astype(np.float32) / 32768.0
    elif sample_width == 3:
        raw = np.frombuffer(frames, dtype=np.uint8).reshape(-1, 3).astype(np.int32)
        samples_i32 = raw[:, 0] | (raw[:, 1] << 8) | (raw[:, 2] << 16)
        samples_i32[samples_i32 >= 0x800000] -= 0x1000000
        samples = samples_i32.astype(np.float32) / 8388608.0
    elif sample_width == 4:
        samples = np.frombuffer(frames, dtype="<i4").astype(np.float32) / 2147483648.0
    else:
        raise RuntimeError(f"Unsupported WAV sample width: {sample_width} bytes")

    if samples.size == 0:
        raise RuntimeError(f"WAV file has no audio samples: {audio_path}")

    waveform = samples.reshape(-1, channels).T
    if channels > 1:
        waveform = waveform.mean(axis=0, keepdims=True)
    return waveform.copy(), sample_rate


def load_audio_for_pipeline(audio_path):
    try:
        import torch
    except Exception as exc:
        raise RuntimeError("torch is required by the pyannote backend") from exc

    waveform, sample_rate = read_pcm_wav(audio_path)
    return {"waveform": torch.from_numpy(waveform), "sample_rate": sample_rate}


def build_payload(output, audio_path, wall_clock_sec):
    annotation = output_annotation(output)
    segments = []
    speakers = []

    for turn, speaker in iter_annotation_segments(annotation):
        name = normalize_speaker_name(speaker)
        if name not in speakers:
            speakers.append(name)
        segments.append(
            {
                "speaker": name,
                "start": float(getattr(turn, "start", 0.0)),
                "end": float(getattr(turn, "end", 0.0)),
                "text": "",
            }
        )

    return {
        "backend": "pyannote-community",
        "model": MODEL_ID,
        "audioPath": str(audio_path),
        "audioDurationSec": audio_duration_sec(audio_path),
        "wallClockSec": wall_clock_sec,
        "speakers": speakers or ["Falante 1"],
        "segments": segments,
    }


def run_backend(audio_path, output_dir, num_speakers=None, min_speakers=None, max_speakers=None):
    token = os.environ.get("HF_TOKEN", "").strip()
    if not token:
        raise RuntimeError(
            "HF_TOKEN is not available. Set it with setx HF_TOKEN <token> and open a new terminal."
        )

    try:
        from pyannote.audio import Pipeline
    except Exception as exc:
        raise RuntimeError(
            "Python package 'pyannote.audio' is not installed. "
            "Run: npm run setup:pyannote"
        ) from exc

    pipeline = Pipeline.from_pretrained(MODEL_ID, token=token)

    kwargs = {}
    if num_speakers is not None:
        kwargs["num_speakers"] = num_speakers
    if min_speakers is not None:
        kwargs["min_speakers"] = min_speakers
    if max_speakers is not None:
        kwargs["max_speakers"] = max_speakers

    started = time.perf_counter()
    output = pipeline(load_audio_for_pipeline(audio_path), **kwargs)
    wall_clock_sec = time.perf_counter() - started

    payload = build_payload(output, audio_path, wall_clock_sec)

    output_dir.mkdir(parents=True, exist_ok=True)
    diarized_path = output_dir / "diarized-transcription.json"
    report_path = output_dir / "pyannote-backend-report.json"
    diarized = {
        "speakers": payload["speakers"],
        "segments": payload["segments"],
    }
    diarized_path.write_text(json.dumps(diarized, ensure_ascii=False, indent=2), encoding="utf-8")
    report = {
        **payload,
        "segmentCount": len(payload["segments"]),
        "speakerCount": len(payload["speakers"]),
        "realtimeFactor": (
            payload["wallClockSec"] / payload["audioDurationSec"]
            if payload["audioDurationSec"] > 0
            else None
        ),
        "speedX": (
            payload["audioDurationSec"] / payload["wallClockSec"]
            if payload["wallClockSec"] > 0
            else None
        ),
        "outputFiles": [str(report_path), str(diarized_path)],
    }
    report_path.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    return report


def parse_args():
    parser = argparse.ArgumentParser(description="Run pyannote Community-1 diarization backend.")
    parser.add_argument("--audio", required=True, help="Audio file path")
    parser.add_argument("--out-dir", required=True, help="Output directory")
    parser.add_argument("--num-speakers", type=int)
    parser.add_argument("--min-speakers", type=int)
    parser.add_argument("--max-speakers", type=int)
    parser.add_argument("--json", action="store_true", help="Print the full JSON report.")
    return parser.parse_args()


if __name__ == "__main__":
    args = parse_args()
    report = run_backend(
        Path(args.audio),
        Path(args.out_dir),
        num_speakers=args.num_speakers,
        min_speakers=args.min_speakers,
        max_speakers=args.max_speakers,
    )
    if args.json:
        print(json.dumps(report, ensure_ascii=False))
    else:
        print(
            json.dumps(
                {
                    "backend": report["backend"],
                    "model": report["model"],
                    "wallClockSec": report["wallClockSec"],
                    "realtimeFactor": report["realtimeFactor"],
                    "speedX": report["speedX"],
                    "speakerCount": report["speakerCount"],
                    "segmentCount": report["segmentCount"],
                    "outputFiles": report["outputFiles"],
                },
                ensure_ascii=False,
            )
        )
