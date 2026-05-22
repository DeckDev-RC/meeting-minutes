import json
import tempfile
from pathlib import Path

from cloud_asr_benchmark import (
    estimate_cost_usd,
    parse_args,
    parse_cloudflare_response,
    parse_deepgram_response,
    parse_xai_response,
    provider_auth_status,
    raise_for_status_with_body,
    run_provider,
    selected_providers,
)


def test_cloudflare_response_parses_wrapped_segments():
    payload = {
        "success": True,
        "result": {
            "text": "Ola Caio.",
            "segments": [{"start": 1.0, "end": 2.5, "text": "Ola Caio."}],
        },
    }

    parsed = parse_cloudflare_response(payload, offset_sec=10.0)

    assert parsed["text"] == "Ola Caio."
    assert parsed["segments"] == [
        {"id": 0, "start": 11.0, "end": 12.5, "text": "Ola Caio."}
    ]


def test_deepgram_response_prefers_utterances_when_available():
    payload = {
        "results": {
            "channels": [{"alternatives": [{"transcript": "fallback"}]}],
            "utterances": [
                {"start": 0.25, "end": 1.5, "transcript": "Boa tarde Caio."},
                {"start": 1.5, "end": 2.0, "transcript": ""},
            ],
        }
    }

    parsed = parse_deepgram_response(payload, offset_sec=30.0)

    assert parsed["text"] == "Boa tarde Caio."
    assert parsed["segments"] == [
        {"id": 0, "start": 30.25, "end": 31.5, "text": "Boa tarde Caio."}
    ]


def test_xai_response_groups_words_and_falls_back_to_text():
    payload = {
        "text": "Ola Manu.",
        "duration": 2.0,
        "words": [
            {"start": 0.0, "end": 0.5, "text": "Ola"},
            {"start": 0.5, "end": 1.0, "text": "Manu."},
        ],
    }

    parsed = parse_xai_response(payload, offset_sec=5.0)

    assert parsed["text"] == "Ola Manu."
    assert parsed["segments"] == [{"id": 0, "start": 5.0, "end": 6.0, "text": "Ola Manu."}]

    fallback = parse_xai_response({"text": "Sem words", "duration": 3.0}, offset_sec=2.0)
    assert fallback["segments"] == [{"id": 0, "start": 2.0, "end": 5.0, "text": "Sem words"}]


def test_cost_estimates_are_per_audio_minute():
    assert estimate_cost_usd("cloudflare", 240 * 60) == 0.1224
    assert estimate_cost_usd("xai", 240 * 60) == 0.4
    assert estimate_cost_usd("deepgram", 240 * 60) == 1.4
    assert estimate_cost_usd("deepgram", 240 * 60, uses_keyterms=True) == 1.712


def test_provider_auth_status_reports_missing_keys_without_values():
    env = {"CLOUDFLARE_ACCOUNT_ID": "abc", "CLOUDFLARE_API_TOKEN": ""}

    status = provider_auth_status(env)

    assert status["cloudflare"] is False
    assert status["deepgram"] is False
    assert status["xai"] is False
    assert json.dumps(status)


def test_provider_auth_status_can_use_windows_fallback_values():
    status = provider_auth_status(
        env={},
        fallback_env={
            "CLOUDFLARE_ACCOUNT_ID": "abc",
            "CLOUDFLARE_API_TOKEN": "token",
        },
    )

    assert status["cloudflare"] is True
    assert status["deepgram"] is False


def test_list_auth_does_not_require_output_directory():
    args = parse_args(["--list-auth"])

    assert args.list_auth is True
    assert args.out_dir is None


def test_default_providers_leave_xai_off_unless_explicitly_selected():
    assert selected_providers(parse_args([])) == ["cloudflare", "deepgram"]
    assert selected_providers(parse_args(["--provider", "xai"])) == ["xai"]


def test_http_error_includes_response_body_without_full_dump():
    class Response:
        status_code = 502
        reason = "Bad Gateway"
        url = "https://api.example.test"
        text = "x" * 520

    try:
        raise_for_status_with_body(Response())
    except RuntimeError as error:
        message = str(error)
    else:
        raise AssertionError("Expected HTTP error")

    assert "502 Bad Gateway" in message
    assert "https://api.example.test" in message
    assert "x" * 300 in message
    assert "x" * 520 not in message


def test_run_provider_accepts_concurrency_and_keeps_chunk_order():
    inputs = [
        {"index": 0, "audioPath": "a.flac", "offsetSec": 0.0, "durationSec": 1.0},
        {"index": 1, "audioPath": "b.flac", "offsetSec": 1.0, "durationSec": 1.0},
    ]

    def fake_transcribe(provider, item, language, keyterms):
        return {
            "index": item["index"],
            "audioPath": item["audioPath"],
            "offsetSec": item["offsetSec"],
            "durationSec": item["durationSec"],
            "wallClockSec": 0.01,
            "speedX": 100.0,
            "estimatedCostUsd": 0.0,
            "text": f"chunk {item['index']}",
            "segments": [
                {
                    "id": 99,
                    "start": item["offsetSec"],
                    "end": item["offsetSec"] + 1.0,
                    "text": f"chunk {item['index']}",
                }
            ],
            "raw": {},
        }

    with tempfile.TemporaryDirectory() as temp_dir:
        report = run_provider(
            "xai",
            inputs,
            Path(temp_dir),
            language="pt",
            keyterms=[],
            concurrency=2,
            transcribe_fn=fake_transcribe,
        )

    assert [chunk["index"] for chunk in report["chunks"]] == [0, 1]
    assert [segment["id"] for segment in report["chunks"][0]["segments"]] == [0]
    assert [segment["id"] for segment in report["chunks"][1]["segments"]] == [1]


if __name__ == "__main__":
    test_cloudflare_response_parses_wrapped_segments()
    test_deepgram_response_prefers_utterances_when_available()
    test_xai_response_groups_words_and_falls_back_to_text()
    test_cost_estimates_are_per_audio_minute()
    test_provider_auth_status_reports_missing_keys_without_values()
    test_provider_auth_status_can_use_windows_fallback_values()
    test_list_auth_does_not_require_output_directory()
    test_default_providers_leave_xai_off_unless_explicitly_selected()
    test_http_error_includes_response_body_without_full_dump()
    test_run_provider_accepts_concurrency_and_keeps_chunk_order()
    print("ok")
