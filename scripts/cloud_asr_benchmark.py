import argparse
import base64
from concurrent.futures import ThreadPoolExecutor
import json
import mimetypes
import os
import time
from pathlib import Path

import requests


PROVIDER_COST_PER_AUDIO_MINUTE = {
    "cloudflare": 0.00051,
    "deepgram": 0.005833333333333333,
    "xai": 0.0016666666666666668,
}

DEFAULT_ACTIVE_PROVIDERS = ["cloudflare", "deepgram"]

KEYTERM_ADDON_COST_PER_AUDIO_MINUTE = {
    "deepgram": 0.0013,
}

DEFAULT_KEYTERMS = [
    "Caio",
    "Manuela",
    "Manu",
    "Renato",
    "Rafaela",
    "Marcos",
    "leitor de documentos",
    "WhatsApp",
    "Drive",
]


def windows_env_value(name):
    if os.name != "nt":
        return None
    try:
        import winreg
    except ImportError:
        return None

    for root, path in (
        (winreg.HKEY_CURRENT_USER, "Environment"),
        (
            winreg.HKEY_LOCAL_MACHINE,
            r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
        ),
    ):
        try:
            with winreg.OpenKey(root, path) as key:
                value, _ = winreg.QueryValueEx(key, name)
                if value:
                    return str(value)
        except OSError:
            continue
    return None


def config_value(name, env=None, fallback_env=None):
    source = os.environ if env is None else env
    value = source.get(name)
    if value:
        return value
    if fallback_env:
        value = fallback_env.get(name)
        if value:
            return value
    if env is None:
        return windows_env_value(name)
    return None


def require_config_value(name):
    value = config_value(name)
    if not value:
        raise RuntimeError(f"Missing required configuration: {name}")
    return value


def chunk_value(chunk, *names, default=None):
    for name in names:
        if name in chunk:
            return chunk[name]
    return default


def provider_auth_status(env=None, fallback_env=None):
    return {
        "cloudflare": bool(config_value("CLOUDFLARE_ACCOUNT_ID", env, fallback_env))
        and bool(config_value("CLOUDFLARE_API_TOKEN", env, fallback_env)),
        "deepgram": bool(config_value("DEEPGRAM_API_KEY", env, fallback_env)),
        "xai": bool(config_value("XAI_API_KEY", env, fallback_env)),
    }


def estimate_cost_usd(provider, duration_sec, uses_keyterms=False):
    minutes = max(0.0, float(duration_sec or 0.0) / 60.0)
    per_minute = PROVIDER_COST_PER_AUDIO_MINUTE[provider]
    if uses_keyterms:
        per_minute += KEYTERM_ADDON_COST_PER_AUDIO_MINUTE.get(provider, 0.0)
    return round(minutes * per_minute, 6)


def clean_text(value):
    return " ".join(str(value or "").split()).strip()


def segment_dict(index, start, end, text):
    text = clean_text(text)
    if not text:
        return None
    start = float(start or 0.0)
    end = float(end or start)
    if end < start:
        end = start
    return {"id": index, "start": start, "end": end, "text": text}


def normalize_segments(raw_segments, offset_sec):
    segments = []
    for raw in raw_segments or []:
        text = raw.get("text") or raw.get("transcript") or raw.get("sentence") or ""
        start = float(raw.get("start", 0.0) or 0.0) + offset_sec
        end = float(raw.get("end", raw.get("duration", 0.0)) or 0.0) + offset_sec
        item = segment_dict(len(segments), start, end, text)
        if item is not None:
            segments.append(item)
    return segments


def parse_cloudflare_response(payload, offset_sec=0.0, duration_sec=0.0):
    body = payload.get("result", payload) if isinstance(payload, dict) else {}
    text = clean_text(body.get("text") or body.get("transcription") or "")
    segments = normalize_segments(body.get("segments", []), offset_sec)
    if not segments and text:
        item = segment_dict(0, offset_sec, offset_sec + float(duration_sec or 0.0), text)
        segments = [item] if item is not None else []
    return {"text": text, "segments": segments, "raw": payload}


def parse_deepgram_response(payload, offset_sec=0.0, duration_sec=0.0):
    results = payload.get("results", {}) if isinstance(payload, dict) else {}
    utterances = results.get("utterances") or payload.get("utterances") or []
    segments = normalize_segments(utterances, offset_sec)

    alternatives = (
        results.get("channels", [{}])[0].get("alternatives", [{}])
        if results.get("channels")
        else [{}]
    )
    text = clean_text((alternatives[0] if alternatives else {}).get("transcript", ""))
    if segments:
        text = clean_text(" ".join(segment["text"] for segment in segments))
    elif text:
        item = segment_dict(0, offset_sec, offset_sec + float(duration_sec or 0.0), text)
        segments = [item] if item is not None else []

    return {"text": text, "segments": segments, "raw": payload}


def group_words(words, offset_sec, max_gap_sec=1.0, max_segment_sec=30.0):
    segments = []
    current = []
    current_start = None
    current_end = None

    for word in words or []:
        text = clean_text(word.get("text") or word.get("word") or "")
        if not text:
            continue
        start = float(word.get("start", current_end or 0.0) or 0.0)
        end = float(word.get("end", start) or start)
        if (
            current
            and current_end is not None
            and (start - current_end > max_gap_sec or end - current_start > max_segment_sec)
        ):
            item = segment_dict(
                len(segments),
                offset_sec + current_start,
                offset_sec + current_end,
                " ".join(current),
            )
            if item is not None:
                segments.append(item)
            current = []
            current_start = None
        if current_start is None:
            current_start = start
        current_end = end
        current.append(text)

    if current:
        item = segment_dict(
            len(segments),
            offset_sec + current_start,
            offset_sec + (current_end if current_end is not None else current_start),
            " ".join(current),
        )
        if item is not None:
            segments.append(item)

    return segments


def parse_xai_response(payload, offset_sec=0.0, duration_sec=0.0):
    text = clean_text(payload.get("text", "") if isinstance(payload, dict) else "")
    response_duration = float(payload.get("duration", duration_sec or 0.0) or 0.0)
    segments = group_words(payload.get("words", []), offset_sec)
    if not segments and text:
        item = segment_dict(0, offset_sec, offset_sec + response_duration, text)
        segments = [item] if item is not None else []
    return {"text": text, "segments": segments, "raw": payload}


def mime_type_for(path):
    guessed = mimetypes.guess_type(str(path))[0]
    if guessed:
        return guessed
    suffix = Path(path).suffix.lower()
    if suffix == ".flac":
        return "audio/flac"
    if suffix == ".wav":
        return "audio/wav"
    if suffix == ".mp4":
        return "video/mp4"
    return "application/octet-stream"


def raise_for_status_with_body(response, body_limit=500):
    if response.status_code < 400:
        return
    body = clean_text(getattr(response, "text", ""))[:body_limit]
    detail = f"HTTP {response.status_code} {response.reason} for {response.url}"
    if body:
        detail += f": {body}"
    raise RuntimeError(detail)


def post_cloudflare(audio_path, language, keyterms):
    account_id = require_config_value("CLOUDFLARE_ACCOUNT_ID")
    token = require_config_value("CLOUDFLARE_API_TOKEN")
    url = (
        "https://api.cloudflare.com/client/v4/accounts/"
        f"{account_id}/ai/run/@cf/openai/whisper-large-v3-turbo"
    )
    audio_base64 = base64.b64encode(Path(audio_path).read_bytes()).decode("utf-8")
    payload = {
        "audio": audio_base64,
        "language": language,
        "task": "transcribe",
        "vad_filter": True,
        "condition_on_previous_text": False,
        "initial_prompt": ", ".join(keyterms),
    }
    response = requests.post(
        url,
        headers={"Authorization": f"Bearer {token}"},
        json=payload,
        timeout=600,
    )
    raise_for_status_with_body(response)
    return response.json()


def post_deepgram(audio_path, language, keyterms):
    token = require_config_value("DEEPGRAM_API_KEY")
    params = [
        ("model", "nova-3"),
        ("language", language),
        ("smart_format", "true"),
        ("punctuate", "true"),
        ("utterances", "true"),
    ]
    for term in keyterms:
        params.append(("keyterm", term))

    with open(audio_path, "rb") as audio:
        response = requests.post(
            "https://api.deepgram.com/v1/listen",
            headers={
                "Authorization": f"Token {token}",
                "Content-Type": mime_type_for(audio_path),
            },
            params=params,
            data=audio,
            timeout=600,
        )
    raise_for_status_with_body(response)
    return response.json()


def post_xai(audio_path, language, keyterms):
    token = require_config_value("XAI_API_KEY")
    data = [("format", "true"), ("language", language)]
    data.extend(("keyterm", term) for term in keyterms)
    with open(audio_path, "rb") as audio:
        response = requests.post(
            "https://api.x.ai/v1/stt",
            headers={"Authorization": f"Bearer {token}"},
            data=data,
            files={"file": (Path(audio_path).name, audio, mime_type_for(audio_path))},
            timeout=600,
        )
    raise_for_status_with_body(response)
    return response.json()


def load_inputs(args):
    if args.chunks_json:
        chunks = json.loads(Path(args.chunks_json).read_text(encoding="utf-8"))
        if not isinstance(chunks, list):
            raise RuntimeError("--chunks-json must contain a JSON array")
        return [
            {
                "index": int(chunk_value(chunk, "index", default=i)),
                "audioPath": chunk_value(chunk, "audioPath", "audio_path"),
                "offsetSec": float(chunk_value(chunk, "offsetSec", "offset_sec", default=0.0) or 0.0),
                "durationSec": float(
                    chunk_value(chunk, "durationSec", "duration_sec", default=0.0) or 0.0
                ),
            }
            for i, chunk in enumerate(chunks)
        ]
    if not args.audio:
        raise SystemExit("Use --audio <path> or --chunks-json <path>")
    return [
        {
            "index": 0,
            "audioPath": args.audio,
            "offsetSec": args.offset_sec,
            "durationSec": args.duration_sec,
        }
    ]


def transcribe_one(provider, item, language, keyterms):
    audio_path = item["audioPath"]
    offset_sec = float(item.get("offsetSec", 0.0) or 0.0)
    duration_sec = float(item.get("durationSec", 0.0) or 0.0)
    started = time.perf_counter()

    if provider == "cloudflare":
        raw = post_cloudflare(audio_path, language, keyterms)
        parsed = parse_cloudflare_response(raw, offset_sec=offset_sec, duration_sec=duration_sec)
    elif provider == "deepgram":
        raw = post_deepgram(audio_path, language, keyterms)
        parsed = parse_deepgram_response(raw, offset_sec=offset_sec, duration_sec=duration_sec)
    elif provider == "xai":
        raw = post_xai(audio_path, language, keyterms)
        parsed = parse_xai_response(raw, offset_sec=offset_sec, duration_sec=duration_sec)
    else:
        raise RuntimeError(f"Unsupported provider: {provider}")

    wall_clock_sec = time.perf_counter() - started
    return {
        "index": item["index"],
        "audioPath": audio_path,
        "offsetSec": offset_sec,
        "durationSec": duration_sec,
        "wallClockSec": wall_clock_sec,
        "speedX": duration_sec / wall_clock_sec if duration_sec > 0 and wall_clock_sec > 0 else None,
        "estimatedCostUsd": estimate_cost_usd(
            provider,
            duration_sec,
            uses_keyterms=bool(keyterms),
        ),
        "text": parsed["text"],
        "segments": parsed["segments"],
        "raw": parsed["raw"],
    }


def run_provider(provider, inputs, out_dir, language, keyterms, concurrency=3, transcribe_fn=transcribe_one):
    provider_dir = out_dir / provider
    provider_dir.mkdir(parents=True, exist_ok=True)
    started = time.perf_counter()
    worker_count = max(1, int(concurrency or 1))

    def run_item(item):
        return transcribe_fn(provider, item, language=language, keyterms=keyterms)

    if worker_count == 1 or len(inputs) <= 1:
        chunks = [run_item(item) for item in inputs]
    else:
        with ThreadPoolExecutor(max_workers=worker_count) as executor:
            chunks = list(executor.map(run_item, inputs))

    wall_clock_sec = time.perf_counter() - started
    audio_duration_sec = sum(float(item.get("durationSec", 0.0) or 0.0) for item in inputs)
    segments = []
    for chunk in chunks:
        segments.extend(chunk["segments"])
    for index, segment in enumerate(segments):
        segment["id"] = index

    report = {
        "provider": provider,
        "language": language,
        "chunkCount": len(chunks),
        "audioDurationSec": audio_duration_sec,
        "wallClockSec": wall_clock_sec,
        "speedX": audio_duration_sec / wall_clock_sec if audio_duration_sec > 0 and wall_clock_sec > 0 else None,
        "estimatedCostUsd": estimate_cost_usd(
            provider,
            audio_duration_sec,
            uses_keyterms=bool(keyterms),
        ),
        "segmentCount": len(segments),
        "textChars": sum(len(chunk["text"]) for chunk in chunks),
        "chunks": chunks,
    }
    (provider_dir / "report.json").write_text(
        json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8"
    )
    (provider_dir / "transcription-segments.json").write_text(
        json.dumps(segments, ensure_ascii=False, indent=2), encoding="utf-8"
    )
    return report


def parse_args(argv=None):
    parser = argparse.ArgumentParser(description="Benchmark cloud ASR providers on app chunks.")
    parser.add_argument("--provider", action="append", choices=["cloudflare", "deepgram", "xai"])
    parser.add_argument("--audio")
    parser.add_argument("--chunks-json")
    parser.add_argument("--out-dir")
    parser.add_argument("--language", default="pt")
    parser.add_argument("--offset-sec", type=float, default=0.0)
    parser.add_argument("--duration-sec", type=float, default=0.0)
    parser.add_argument("--keyterm", action="append", default=[])
    parser.add_argument("--concurrency", type=int, default=3)
    parser.add_argument("--list-auth", action="store_true")
    return parser.parse_args(argv)


def selected_providers(args):
    return args.provider or list(DEFAULT_ACTIVE_PROVIDERS)


def main():
    args = parse_args()
    if args.list_auth:
        print(json.dumps(provider_auth_status(), ensure_ascii=False, indent=2))
        return 0

    if not args.out_dir:
        raise SystemExit("Use --out-dir <path> to store benchmark reports.")

    providers = selected_providers(args)
    auth = provider_auth_status()
    missing = [provider for provider in providers if not auth[provider]]
    if missing:
        raise SystemExit(
            "Missing credentials for: "
            + ", ".join(missing)
            + ". Required env vars: CLOUDFLARE_ACCOUNT_ID/CLOUDFLARE_API_TOKEN, "
            + "DEEPGRAM_API_KEY, XAI_API_KEY."
        )

    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    keyterms = DEFAULT_KEYTERMS + list(args.keyterm or [])
    inputs = load_inputs(args)
    summary = [
        run_provider(
            provider,
            inputs,
            out_dir,
            language=args.language,
            keyterms=keyterms,
            concurrency=args.concurrency,
        )
        for provider in providers
    ]
    (out_dir / "summary.json").write_text(
        json.dumps(summary, ensure_ascii=False, indent=2), encoding="utf-8"
    )
    print(json.dumps(summary, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
