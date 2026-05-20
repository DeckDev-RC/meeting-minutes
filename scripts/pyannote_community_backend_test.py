import tempfile
import wave
from pathlib import Path
from types import SimpleNamespace

from pyannote_community_backend import build_payload, normalize_speaker_name, read_pcm_wav


def test_normalize_speaker_name_handles_pyannote_labels():
    assert normalize_speaker_name("SPEAKER_00") == "Falante 1"
    assert normalize_speaker_name("SPEAKER_02") == "Falante 3"
    assert normalize_speaker_name("speaker_1") == "Falante 2"
    assert normalize_speaker_name("") == "Falante 1"


def test_build_payload_uses_exclusive_diarization_when_available():
    output = SimpleNamespace(
        speaker_diarization=[
            (SimpleNamespace(start=0.0, end=1.0), "SPEAKER_00"),
        ],
        exclusive_speaker_diarization=[
            (SimpleNamespace(start=0.0, end=1.5), "SPEAKER_00"),
            (SimpleNamespace(start=1.5, end=3.0), "SPEAKER_01"),
        ],
    )

    payload = build_payload(output, "meeting.wav", 4.2)

    assert payload["backend"] == "pyannote-community"
    assert payload["model"] == "pyannote/speaker-diarization-community-1"
    assert payload["speakers"] == ["Falante 1", "Falante 2"]
    assert payload["segments"][0]["start"] == 0.0
    assert payload["segments"][1]["speaker"] == "Falante 2"


def test_read_pcm_wav_avoids_torchcodec_loader():
    with tempfile.TemporaryDirectory() as tmp:
        wav_path = Path(tmp) / "sample.wav"
        with wave.open(str(wav_path), "wb") as handle:
            handle.setnchannels(1)
            handle.setsampwidth(2)
            handle.setframerate(16000)
            handle.writeframes((0).to_bytes(2, "little", signed=True))
            handle.writeframes((16384).to_bytes(2, "little", signed=True))

        waveform, sample_rate = read_pcm_wav(wav_path)
        assert sample_rate == 16000
        assert waveform.shape == (1, 2)
        assert abs(float(waveform[0][0])) < 0.0001
        assert 0.49 < float(waveform[0][1]) < 0.51


if __name__ == "__main__":
    test_normalize_speaker_name_handles_pyannote_labels()
    test_build_payload_uses_exclusive_diarization_when_available()
    test_read_pcm_wav_avoids_torchcodec_loader()
    print("ok")
