import json
import tempfile
from pathlib import Path
from types import SimpleNamespace

from diarize_cpu_backend import (
    ReusableDiarizeEngine,
    build_payload,
    compact_report,
    dependency_warnings,
    run_backend_batch_with_diarize,
)


def test_build_payload_normalizes_speaker_names():
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


def test_run_backend_batch_reuses_loaded_diarize_function():
    calls = []

    def fake_diarize(audio_path, **kwargs):
        calls.append((audio_path, kwargs))
        speaker = "SPEAKER_00" if audio_path.endswith("a.wav") else "SPEAKER_01"
        return SimpleNamespace(
            audio_duration=10.0,
            segments=[SimpleNamespace(start=0.0, end=2.0, speaker=speaker)],
        )

    chunks = [
        {"index": 0, "audioPath": "a.wav", "offsetSec": 0.0},
        {"index": 1, "audioPath": "b.wav", "offsetSec": 10.0},
    ]

    with tempfile.TemporaryDirectory() as tmp:
        report = run_backend_batch_with_diarize(
            fake_diarize,
            chunks,
            Path(tmp),
            num_speakers=2,
        )
        saved = json.loads((Path(tmp) / "chunk-diarized-results.json").read_text())

    assert [call[0] for call in calls] == ["a.wav", "b.wav"]
    assert all(call[1] == {"num_speakers": 2} for call in calls)
    assert report["chunkCount"] == 2
    assert report["audioDurationSec"] == 20.0
    assert report["speakerCount"] == 2
    assert report["segmentCount"] == 2
    assert report["realtimeFactor"] is not None
    assert report["speedX"] is not None
    assert saved[0]["index"] == 0
    assert saved[1]["diarized"]["segments"][0]["speaker"] == "Falante 2"


def test_compact_report_supports_batch_reports():
    report = {
        "backend": "diarize",
        "model": "diarize-0.1.2",
        "wallClockSec": 4.0,
        "audioDurationSec": 20.0,
        "realtimeFactor": 0.2,
        "speedX": 5.0,
        "speakerCount": 2,
        "segmentCount": 4,
        "outputFiles": ["report.json"],
    }

    compact = compact_report(report)

    assert compact["realtimeFactor"] == 0.2
    assert compact["speedX"] == 5.0
    assert compact["speakerCount"] == 2
    assert compact["segmentCount"] == 4


def test_payload_includes_speaker_centroids_when_available():
    result = SimpleNamespace(
        audio_duration=12.5,
        segments=[SimpleNamespace(start=0.5, end=2.0, speaker="SPEAKER_00")],
        speaker_centroids={"SPEAKER_00": [1.0, 0.0]},
    )

    payload = build_payload(result, "diarize", "diarize-0.1.2", "meeting.wav", 1.25)

    assert payload["speakerCentroids"] == [
        {"speaker": "Falante 1", "embedding": [1.0, 0.0]}
    ]


def test_reusable_engine_constructs_speaker_once():
    calls = []

    def speaker_factory():
        speaker = object()
        calls.append(speaker)
        return speaker

    engine = ReusableDiarizeEngine(speaker_factory=speaker_factory)

    assert engine.speaker is engine.speaker
    assert calls == [engine.speaker]


def test_dependency_warnings_flag_torchaudio_29():
    warnings = dependency_warnings({"torchaudio": "2.9.0", "torch": "2.9.0"})

    assert warnings
    assert "torchaudio>=2.9" in warnings[0]


if __name__ == "__main__":
    test_build_payload_normalizes_speaker_names()
    test_run_backend_batch_reuses_loaded_diarize_function()
    test_compact_report_supports_batch_reports()
    test_payload_includes_speaker_centroids_when_available()
    test_reusable_engine_constructs_speaker_once()
    test_dependency_warnings_flag_torchaudio_29()
    print("ok")
