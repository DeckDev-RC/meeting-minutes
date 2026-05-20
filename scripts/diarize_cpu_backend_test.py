from types import SimpleNamespace

from diarize_cpu_backend import build_payload


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


if __name__ == "__main__":
    test_build_payload_normalizes_speaker_names()
    print("ok")
