import json
import tempfile
from pathlib import Path
from types import SimpleNamespace

from meeting_minutes_sidecar import (
    build_diarization_response,
    build_transcription_response,
    command_health,
    diarized_segments_to_turns,
    run_diarize_modern_cpu_request,
    run_transcribe_local_request,
    speaker_label_to_index,
    transcription_segments_from_report,
)


def test_health_contract_is_json_safe():
    response = command_health()

    assert response["ok"] is True
    assert "health" in response["commands"]
    assert "transcribe-local" in response["commands"]
    assert "diarize-modern-cpu" in response["commands"]
    json.dumps(response)


def test_speaker_labels_convert_to_zero_based_indexes():
    assert speaker_label_to_index("SPEAKER_00") == 0
    assert speaker_label_to_index("SPEAKER_01") == 1
    assert speaker_label_to_index("Falante 3") == 2
    assert speaker_label_to_index("") == 0


def test_diarized_segments_to_turns_applies_chunk_offset():
    turns = diarized_segments_to_turns(
        [
            {"start": 0.5, "end": 2.0, "speaker": "Falante 2"},
            {"start": 2.0, "end": 1.5, "speaker": "SPEAKER_00"},
        ],
        offset_sec=10.0,
    )

    assert turns == [
        {"start": 10.5, "end": 12.0, "speakerIndex": 1},
        {"start": 12.0, "end": 12.0, "speakerIndex": 0},
    ]


def test_transcription_response_keeps_operational_telemetry():
    response = build_transcription_response(
        {
            "backend": "faster-whisper",
            "model": "turbo",
            "device": "cpu",
            "computeType": "int8",
            "segments": [{"id": 0, "start": 0.0, "end": 1.0, "text": "ola"}],
            "wallClockSec": 1.25,
            "segmentCount": 1,
            "speedX": 12.0,
        },
        command_wall_clock_sec=1.5,
    )

    assert response["ok"] is True
    assert response["segments"][0]["text"] == "ola"
    assert response["telemetry"]["backend"] == "sidecar-faster-whisper"
    assert response["telemetry"]["model"] == "turbo"
    assert response["telemetry"]["commandWallClockSec"] == 1.5


def test_transcription_response_flattens_batch_chunk_segments():
    segments = transcription_segments_from_report(
        {"backend": "faster-whisper", "segmentCount": 2},
        chunk_outputs=[
            {"segments": [{"id": 99, "start": 0.0, "end": 1.0, "text": "um"}]},
            {"segments": [{"id": 99, "start": 10.0, "end": 11.0, "text": "dois"}]},
        ],
    )

    assert segments == [
        {"id": 0, "start": 0.0, "end": 1.0, "text": "um"},
        {"id": 1, "start": 10.0, "end": 11.0, "text": "dois"},
    ]


def test_diarization_response_builds_sdd_turn_contract():
    response = build_diarization_response(
        {
            "backend": "diarize",
            "model": "diarize-0.1.2",
            "segments": [{"start": 0.0, "end": 1.5, "speaker": "Falante 1"}],
            "speakers": ["Falante 1"],
            "wallClockSec": 2.0,
            "speakerCount": 1,
            "segmentCount": 1,
        },
        command_wall_clock_sec=2.2,
    )

    assert response["ok"] is True
    assert response["turns"] == [{"start": 0.0, "end": 1.5, "speakerIndex": 0}]
    assert response["telemetry"]["backend"] == "sidecar-modern-cpu"
    assert response["telemetry"]["wallClockSec"] == 2.0


def test_transcribe_local_request_uses_batch_backend_without_real_model():
    calls = []

    class FakeEngine:
        def __init__(self, **kwargs):
            self.kwargs = kwargs

    def run_backend_batch_with_engine(engine, chunks, output_dir):
        calls.append((engine.kwargs, chunks, Path(output_dir)))
        Path(output_dir).mkdir(parents=True, exist_ok=True)
        chunks_path = Path(output_dir) / "chunk-transcription-results.json"
        chunks_path.write_text(
            json.dumps(
                [
                    {
                        "index": 0,
                        "segments": [
                            {"id": 0, "start": 0.0, "end": 1.0, "text": "ola"}
                        ],
                    },
                    {
                        "index": 1,
                        "segments": [
                            {"id": 0, "start": 10.0, "end": 11.0, "text": "mundo"}
                        ],
                    },
                ]
            ),
            encoding="utf-8",
        )
        return {
            "backend": "faster-whisper",
            "model": engine.kwargs["model"],
            "device": "cpu",
            "computeType": "int8",
            "chunkCount": len(chunks),
            "audioDurationSec": sum(chunk["durationSec"] for chunk in chunks),
            "wallClockSec": 1.0,
            "segmentCount": 2,
            "speedX": 20.0,
            "outputFiles": [str(chunks_path)],
        }

    fake_module = SimpleNamespace(
        FasterWhisperEngine=FakeEngine,
        run_backend_batch_with_engine=run_backend_batch_with_engine,
        run_backend_with_engine=None,
    )

    with tempfile.TemporaryDirectory() as tmp:
        response = run_transcribe_local_request(
            {
                "chunks": [
                    {"index": 0, "audioPath": "a.flac", "offsetSec": 0.0, "durationSec": 10.0},
                    {"index": 1, "audioPath": "b.flac", "offsetSec": 10.0, "durationSec": 10.0},
                ],
                "outputDir": tmp,
                "model": "turbo",
            },
            backend_module=fake_module,
        )

    assert calls[0][0]["model"] == "turbo"
    assert response["ok"] is True
    assert [segment["text"] for segment in response["segments"]] == ["ola", "mundo"]
    assert response["telemetry"]["chunkCount"] == 2
    assert response["telemetry"]["speedX"] == 20.0


def test_diarize_modern_cpu_request_uses_batch_backend_without_real_model():
    calls = []

    def fake_load_diarize_function():
        return object()

    def run_backend_batch_with_diarize(
        diarize_fn,
        chunks,
        output_dir,
        num_speakers=None,
        min_speakers=None,
        max_speakers=None,
        max_workers=1,
        diarize_factory=None,
        embedding_profile=None,
    ):
        calls.append(
            {
                "diarizeFn": diarize_fn,
                "chunks": chunks,
                "outputDir": Path(output_dir),
                "numSpeakers": num_speakers,
                "minSpeakers": min_speakers,
                "maxSpeakers": max_speakers,
                "maxWorkers": max_workers,
                "factory": diarize_factory,
                "embeddingProfile": embedding_profile,
            }
        )
        return {
            "backend": "diarize",
            "model": "diarize-0.1.2",
            "chunkCount": len(chunks),
            "wallClockSec": 1.5,
            "speakerCount": 2,
            "segmentCount": 2,
            "outputFiles": [],
        }

    fake_module = SimpleNamespace(
        load_diarize_function=fake_load_diarize_function,
        run_backend_batch_with_diarize=run_backend_batch_with_diarize,
        run_backend_with_diarize=None,
    )

    with tempfile.TemporaryDirectory() as tmp:
        response = run_diarize_modern_cpu_request(
            {
                "chunks": [
                    {"index": 0, "audioPath": "a.wav", "offsetSec": 0.0},
                    {"index": 1, "audioPath": "b.wav", "offsetSec": 10.0},
                ],
                "outputDir": tmp,
                "expectedSpeakers": 2,
                "numThreads": 2,
                "embeddingProfile": "balanced",
            },
            backend_module=fake_module,
        )

    assert calls[0]["numSpeakers"] == 2
    assert calls[0]["maxWorkers"] == 2
    assert calls[0]["embeddingProfile"] == "balanced"
    assert response["ok"] is True
    assert response["telemetry"]["backend"] == "sidecar-modern-cpu"


if __name__ == "__main__":
    test_health_contract_is_json_safe()
    test_speaker_labels_convert_to_zero_based_indexes()
    test_diarized_segments_to_turns_applies_chunk_offset()
    test_transcription_response_keeps_operational_telemetry()
    test_transcription_response_flattens_batch_chunk_segments()
    test_diarization_response_builds_sdd_turn_contract()
    test_transcribe_local_request_uses_batch_backend_without_real_model()
    test_diarize_modern_cpu_request_uses_batch_backend_without_real_model()
    print("ok")
