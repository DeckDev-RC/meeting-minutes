import json
import tempfile
from pathlib import Path
from types import SimpleNamespace

from transcribe_faster_whisper_backend import (
    build_payload,
    compact_report,
    normalize_model_name,
    run_backend_batch_with_engine,
)


def test_model_aliases_match_faster_whisper_names():
    assert normalize_model_name("whisper-large-v3-turbo") == "turbo"
    assert normalize_model_name("large-v3-turbo") == "turbo"
    assert normalize_model_name("large-v3") == "large-v3"


def test_build_payload_offsets_and_trims_segments():
    info = SimpleNamespace(language="pt", language_probability=0.92)
    segments = [
        SimpleNamespace(start=0.5, end=2.0, text="  ola mundo  "),
        SimpleNamespace(start=2.0, end=4.0, text=""),
    ]

    payload = build_payload(
        segments,
        info,
        "chunk.flac",
        offset_sec=10.0,
        model="turbo",
        device="cpu",
        compute_type="int8",
        wall_clock_sec=1.25,
    )

    assert payload["segments"] == [
        {"id": 0, "start": 10.5, "end": 12.0, "text": "ola mundo"}
    ]
    assert payload["language"] == "pt"
    assert payload["languageProbability"] == 0.92


def test_batch_reuses_loaded_engine_for_chunks_and_writes_outputs():
    calls = []

    class FakeEngine:
        model = "turbo"
        device = "cpu"
        compute_type = "int8"

        def transcribe(self, audio_path):
            calls.append(audio_path)
            info = SimpleNamespace(language="pt", language_probability=0.9)
            segment = SimpleNamespace(start=0.0, end=1.0, text=Path(audio_path).stem)
            return [segment], info

    chunks = [
        {"index": 0, "audioPath": "a.flac", "offsetSec": 0.0, "durationSec": 10.0},
        {"index": 1, "audioPath": "b.flac", "offsetSec": 10.0, "durationSec": 11.0},
    ]

    with tempfile.TemporaryDirectory() as tmp:
        report = run_backend_batch_with_engine(FakeEngine(), chunks, Path(tmp))
        saved = json.loads((Path(tmp) / "chunk-transcription-results.json").read_text())

    assert calls == ["a.flac", "b.flac"]
    assert report["chunkCount"] == 2
    assert report["audioDurationSec"] == 21.0
    assert report["segmentCount"] == 2
    assert report["speedX"] is not None
    assert saved[1]["segments"][0]["start"] == 10.0
    assert saved[1]["segments"][0]["text"] == "b"


def test_compact_report_keeps_operational_fields():
    report = {
        "backend": "faster-whisper",
        "model": "turbo",
        "device": "cpu",
        "computeType": "int8",
        "chunkCount": 2,
        "audioDurationSec": 20.0,
        "wallClockSec": 4.0,
        "realtimeFactor": 0.2,
        "speedX": 5.0,
        "segmentCount": 6,
        "outputFiles": ["report.json"],
    }

    compact = compact_report(report)

    assert compact["model"] == "turbo"
    assert compact["device"] == "cpu"
    assert compact["computeType"] == "int8"
    assert compact["speedX"] == 5.0


if __name__ == "__main__":
    test_model_aliases_match_faster_whisper_names()
    test_build_payload_offsets_and_trims_segments()
    test_batch_reuses_loaded_engine_for_chunks_and_writes_outputs()
    test_compact_report_keeps_operational_fields()
    print("ok")
