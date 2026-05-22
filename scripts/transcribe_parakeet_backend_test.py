import json
import tempfile
from pathlib import Path

from transcribe_parakeet_backend import (
    DEFAULT_MODEL,
    collapse_repeated_phrases,
    compact_report,
    normalize_model_name,
    run_backend_batch_with_engine,
    trim_repeated_prefix,
)


def test_model_aliases_match_parakeet_hf_id():
    assert normalize_model_name("parakeet") == DEFAULT_MODEL
    assert normalize_model_name("parakeet-tdt-0.6b-v3") == DEFAULT_MODEL
    assert normalize_model_name("custom/model") == "custom/model"
    assert normalize_model_name(None) == DEFAULT_MODEL


def test_trim_repeated_prefix_removes_window_boundary_echo():
    assert (
        trim_repeated_prefix(
            "Caio explicou o fluxo de documentos",
            "fluxo de documentos e proximos passos",
        )
        == "e proximos passos"
    )
    assert trim_repeated_prefix("sem repeticao aqui", "novo assunto") == "novo assunto"


def test_collapse_repeated_phrases_limits_parakeet_loops():
    text = "isso e um teste " + "de repeticao curta " * 8 + "fim"

    assert collapse_repeated_phrases(text) == "isso e um teste de repeticao curta fim"


def test_batch_reuses_loaded_engine_for_chunks_and_writes_outputs():
    calls = []

    class FakeEngine:
        model = DEFAULT_MODEL
        device = "cpu"
        batch_size = 4
        window_sec = 30.0
        overlap_sec = 1.0

        def transcribe_file(self, audio_path, offset_sec):
            calls.append(str(audio_path))
            return (
                [
                    {
                        "id": 0,
                        "start": offset_sec,
                        "end": offset_sec + 1.0,
                        "text": Path(audio_path).stem,
                    }
                ],
                {"windowCount": 1, "audioDurationSec": 1.0},
            )

    chunks = [
        {"index": 0, "audioPath": "a.flac", "offsetSec": 0.0, "durationSec": 10.0},
        {"index": 1, "audioPath": "b.flac", "offsetSec": 10.0, "durationSec": 11.0},
    ]

    with tempfile.TemporaryDirectory() as tmp:
        report = run_backend_batch_with_engine(FakeEngine(), chunks, Path(tmp))
        saved = json.loads((Path(tmp) / "chunk-transcription-results.json").read_text())

    assert calls == ["a.flac", "b.flac"]
    assert report["chunkCount"] == 2
    assert report["windowCount"] == 2
    assert report["audioDurationSec"] == 21.0
    assert report["segmentCount"] == 2
    assert report["speedX"] is not None
    assert saved[1]["segments"][0]["start"] == 10.0
    assert saved[1]["segments"][0]["text"] == "b"


def test_compact_report_keeps_operational_fields():
    report = {
        "backend": "parakeet-tdt-windowed",
        "model": DEFAULT_MODEL,
        "device": "cpu",
        "batchSize": 4,
        "windowSec": 30.0,
        "overlapSec": 1.0,
        "chunkCount": 2,
        "windowCount": 8,
        "audioDurationSec": 20.0,
        "wallClockSec": 4.0,
        "realtimeFactor": 0.2,
        "speedX": 5.0,
        "segmentCount": 6,
        "outputFiles": ["report.json"],
    }

    compact = compact_report(report)

    assert compact["model"] == DEFAULT_MODEL
    assert compact["batchSize"] == 4
    assert compact["windowSec"] == 30.0
    assert compact["overlapSec"] == 1.0
    assert compact["speedX"] == 5.0


if __name__ == "__main__":
    test_model_aliases_match_parakeet_hf_id()
    test_trim_repeated_prefix_removes_window_boundary_echo()
    test_collapse_repeated_phrases_limits_parakeet_loops()
    test_batch_reuses_loaded_engine_for_chunks_and_writes_outputs()
    test_compact_report_keeps_operational_fields()
    print("ok")
