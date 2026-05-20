import json
import argparse
import time
from pathlib import Path
from types import SimpleNamespace


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


def segment_to_dict(segment):
    return {
        "speaker": normalize_speaker_name(getattr(segment, "speaker", "")),
        "start": float(getattr(segment, "start", 0.0)),
        "end": float(getattr(segment, "end", 0.0)),
        "text": "",
    }


def build_payload(result, backend, model, audio_path, wall_clock_sec):
    segments = [segment_to_dict(segment) for segment in getattr(result, "segments", [])]
    speakers = []
    for segment in segments:
        if segment["speaker"] not in speakers:
            speakers.append(segment["speaker"])

    return {
        "backend": backend,
        "model": model,
        "audioPath": audio_path,
        "audioDurationSec": float(getattr(result, "audio_duration", 0.0) or 0.0),
        "wallClockSec": wall_clock_sec,
        "speakers": speakers or ["Falante 1"],
        "segments": segments,
    }


def run_backend(audio_path, output_dir, num_speakers=None, min_speakers=None, max_speakers=None):
    try:
        from diarize import diarize
    except Exception as exc:
        raise RuntimeError(
            "Python package 'diarize' is not installed. "
            "Install it in the selected environment with: python -m pip install diarize"
        ) from exc

    kwargs = {}
    if num_speakers is not None:
        kwargs["num_speakers"] = num_speakers
    if min_speakers is not None:
        kwargs["min_speakers"] = min_speakers
    if max_speakers is not None:
        kwargs["max_speakers"] = max_speakers

    started = time.perf_counter()
    result = diarize(str(audio_path), **kwargs)
    wall_clock_sec = time.perf_counter() - started

    payload = build_payload(
        result,
        backend="diarize",
        model="diarize-0.1.2",
        audio_path=str(audio_path),
        wall_clock_sec=wall_clock_sec,
    )

    output_dir.mkdir(parents=True, exist_ok=True)
    diarized_path = output_dir / "diarized-transcription.json"
    report_path = output_dir / "diarize-backend-report.json"
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
    parser = argparse.ArgumentParser(description="Run CPU-only diarize backend benchmark.")
    parser.add_argument("--audio", required=True, help="Audio file path")
    parser.add_argument("--out-dir", required=True, help="Output directory")
    parser.add_argument("--num-speakers", type=int)
    parser.add_argument("--min-speakers", type=int)
    parser.add_argument("--max-speakers", type=int)
    parser.add_argument("--json", action="store_true", help="Print the full JSON report.")
    return parser.parse_args()


def _test_build_payload():
    result = SimpleNamespace(
        audio_duration=12.5,
        segments=[
            SimpleNamespace(start=0.5, end=2.0, speaker="SPEAKER_00"),
            SimpleNamespace(start=2.0, end=4.0, speaker="SPEAKER_01"),
        ],
    )

    payload = build_payload(result, "diarize", "diarize-0.1.2", "meeting.wav", 1.25)

    assert payload["speakers"] == ["Falante 1", "Falante 2"]
    assert payload["segments"][0]["speaker"] == "Falante 1"
    assert payload["segments"][1]["speaker"] == "Falante 2"
    assert payload["audioDurationSec"] == 12.5


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
