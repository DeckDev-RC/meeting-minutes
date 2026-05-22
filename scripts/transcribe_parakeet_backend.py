import argparse
import json
import math
import time
from dataclasses import dataclass
from pathlib import Path


DEFAULT_MODEL = "nvidia/parakeet-tdt-0.6b-v3"
MODEL_ALIASES = {
    "parakeet": DEFAULT_MODEL,
    "parakeet-tdt": DEFAULT_MODEL,
    "parakeet-tdt-0.6b": DEFAULT_MODEL,
    "parakeet-tdt-0.6b-v3": DEFAULT_MODEL,
}


@dataclass
class AudioWindow:
    start_sec: float
    end_sec: float
    samples: object


def normalize_model_name(value):
    raw = str(value or DEFAULT_MODEL).strip()
    return MODEL_ALIASES.get(raw.lower(), raw or DEFAULT_MODEL)


def positive_int(value, fallback, minimum=1, maximum=None):
    try:
        parsed = int(value)
    except (TypeError, ValueError):
        return fallback
    parsed = max(minimum, parsed)
    if maximum is not None:
        parsed = min(maximum, parsed)
    return parsed


def positive_float(value, fallback, minimum=0.0, maximum=None):
    try:
        parsed = float(value)
    except (TypeError, ValueError):
        return fallback
    if not math.isfinite(parsed):
        return fallback
    parsed = max(minimum, parsed)
    if maximum is not None:
        parsed = min(maximum, parsed)
    return parsed


def resolve_device(value):
    raw = str(value or "auto").strip().lower()
    if raw in {"cpu", "cuda"}:
        return raw
    try:
        import torch

        return "cuda" if torch.cuda.is_available() else "cpu"
    except Exception:
        return "cpu"


def chunk_value(chunk, *names, default=None):
    for name in names:
        if name in chunk:
            return chunk[name]
    return default


def read_audio(audio_path, target_sample_rate):
    try:
        import numpy as np
        import soundfile as sf
    except Exception as exc:
        raise RuntimeError(
            "Python packages 'numpy' and 'soundfile' are required. "
            "Install them with: npm run setup:transcribe-parakeet"
        ) from exc

    samples, sample_rate = sf.read(str(audio_path), dtype="float32", always_2d=False)
    if getattr(samples, "ndim", 1) > 1:
        samples = samples.mean(axis=1)
    samples = np.asarray(samples, dtype="float32")
    if int(sample_rate) == int(target_sample_rate):
        return samples, int(sample_rate)

    try:
        from scipy.signal import resample_poly
    except Exception as exc:
        raise RuntimeError(
            f"Audio is {sample_rate} Hz, but Parakeet expects {target_sample_rate} Hz. "
            "The app normally exports 16 kHz chunks; install scipy only if direct "
            "non-16 kHz files must be resampled in this backend."
        ) from exc

    gcd = math.gcd(int(sample_rate), int(target_sample_rate))
    up = int(target_sample_rate) // gcd
    down = int(sample_rate) // gcd
    return resample_poly(samples, up, down).astype("float32"), int(target_sample_rate)


def iter_audio_windows(samples, sample_rate, window_sec, overlap_sec):
    duration_sec = len(samples) / float(sample_rate) if sample_rate > 0 else 0.0
    if duration_sec <= 0:
        return []

    window_sec = positive_float(window_sec, 30.0, minimum=1.0)
    overlap_sec = positive_float(overlap_sec, 0.0, minimum=0.0, maximum=window_sec - 0.1)
    step_sec = max(0.1, window_sec - overlap_sec)
    windows = []
    start_sec = 0.0

    while start_sec < duration_sec:
        end_sec = min(start_sec + window_sec, duration_sec)
        if end_sec - start_sec <= 0.05:
            break
        start_sample = max(0, int(round(start_sec * sample_rate)))
        end_sample = min(len(samples), int(round(end_sec * sample_rate)))
        if end_sample > start_sample:
            windows.append(AudioWindow(start_sec, end_sec, samples[start_sample:end_sample]))
        if end_sec >= duration_sec:
            break
        start_sec += step_sec

    return windows


def trim_repeated_prefix(previous_text, current_text, max_words=12):
    previous_words = str(previous_text or "").split()
    current_words = str(current_text or "").split()
    if not previous_words or not current_words:
        return str(current_text or "").strip()

    limit = min(max_words, len(previous_words), len(current_words))
    for size in range(limit, 0, -1):
        left = " ".join(previous_words[-size:]).casefold()
        right = " ".join(current_words[:size]).casefold()
        if left == right:
            return " ".join(current_words[size:]).strip()
    return str(current_text or "").strip()


def collapse_repeated_phrases(text, max_phrase_words=8, min_repeats=4):
    words = str(text or "").split()
    if len(words) < min_repeats:
        return str(text or "").strip()

    collapsed = []
    index = 0
    while index < len(words):
        matched = False
        max_size = min(max_phrase_words, (len(words) - index) // min_repeats)
        for size in range(1, max_size + 1):
            phrase = words[index : index + size]
            repeats = 1
            while index + (repeats + 1) * size <= len(words):
                next_phrase = words[
                    index + repeats * size : index + (repeats + 1) * size
                ]
                if [word.casefold() for word in next_phrase] != [
                    word.casefold() for word in phrase
                ]:
                    break
                repeats += 1
            if repeats >= min_repeats:
                collapsed.extend(phrase)
                index += repeats * size
                matched = True
                break
        if not matched:
            collapsed.append(words[index])
            index += 1

    return " ".join(collapsed).strip()


def decoded_texts(processor, output):
    decoded = processor.decode(output.sequences, skip_special_tokens=True)
    if isinstance(decoded, tuple):
        decoded = decoded[0]
    if isinstance(decoded, str):
        return [decoded]
    return [str(item or "") for item in decoded]


def segment_dict(segment_id, start_sec, end_sec, text):
    text = collapse_repeated_phrases(str(text or "").strip())
    if not text:
        return None
    if end_sec < start_sec:
        end_sec = start_sec
    return {
        "id": segment_id,
        "start": float(start_sec),
        "end": float(end_sec),
        "text": text,
    }


class ParakeetEngine:
    def __init__(
        self,
        model=DEFAULT_MODEL,
        device="auto",
        batch_size=4,
        window_sec=30.0,
        overlap_sec=1.0,
    ):
        self.model = normalize_model_name(model)
        self.device = resolve_device(device)
        self.batch_size = positive_int(batch_size, 4, minimum=1, maximum=16)
        self.window_sec = positive_float(window_sec, 30.0, minimum=1.0)
        self.overlap_sec = positive_float(overlap_sec, 1.0, minimum=0.0, maximum=self.window_sec - 0.1)
        self._processor = None
        self._model = None

    @property
    def processor(self):
        if self._processor is None:
            try:
                from transformers import AutoProcessor
            except Exception as exc:
                raise RuntimeError(
                    "Python package 'transformers' with Parakeet support is not installed. "
                    "Install it with: npm run setup:transcribe-parakeet"
                ) from exc
            self._processor = AutoProcessor.from_pretrained(self.model)
        return self._processor

    @property
    def tdt_model(self):
        if self._model is None:
            try:
                from transformers import AutoModelForTDT
            except Exception as exc:
                raise RuntimeError(
                    "AutoModelForTDT is unavailable. Install Transformers from source with "
                    "npm run setup:transcribe-parakeet."
                ) from exc
            self._model = AutoModelForTDT.from_pretrained(
                self.model,
                dtype="auto",
                device_map=self.device,
            )
            self._model.eval()
        return self._model

    @property
    def sample_rate(self):
        return int(getattr(self.processor.feature_extractor, "sampling_rate", 16000) or 16000)

    def transcribe_windows(self, windows):
        import torch

        segments = []
        previous_text = ""
        for batch_start in range(0, len(windows), self.batch_size):
            batch = windows[batch_start : batch_start + self.batch_size]
            speech_samples = [window.samples for window in batch]
            inputs = self.processor(
                speech_samples,
                sampling_rate=self.sample_rate,
                return_tensors="pt",
                padding=True,
            )
            inputs.to(device=self.tdt_model.device, dtype=self.tdt_model.dtype)
            with torch.inference_mode():
                output = self.tdt_model.generate(**inputs, return_dict_in_generate=True)

            for window, text in zip(batch, decoded_texts(self.processor, output)):
                text = trim_repeated_prefix(previous_text, text)
                previous_text = f"{previous_text} {text}".strip() if text else previous_text
                item = segment_dict(len(segments), window.start_sec, window.end_sec, text)
                if item is not None:
                    segments.append(item)
        return segments

    def transcribe_file(self, audio_path, offset_sec):
        samples, sample_rate = read_audio(audio_path, self.sample_rate)
        windows = iter_audio_windows(samples, sample_rate, self.window_sec, self.overlap_sec)
        segments = self.transcribe_windows(windows)
        for segment in segments:
            segment["start"] += float(offset_sec)
            segment["end"] += float(offset_sec)
        return segments, {
            "windowCount": len(windows),
            "audioDurationSec": len(samples) / float(sample_rate) if sample_rate else 0.0,
        }


def transcribe_one(engine, audio_path, offset_sec):
    started = time.perf_counter()
    segments, stats = engine.transcribe_file(audio_path, offset_sec)
    wall_clock_sec = time.perf_counter() - started
    return {
        "backend": "parakeet-tdt-windowed",
        "model": engine.model,
        "device": engine.device,
        "batchSize": engine.batch_size,
        "windowSec": engine.window_sec,
        "overlapSec": engine.overlap_sec,
        "audioPath": str(audio_path),
        "offsetSec": float(offset_sec),
        "wallClockSec": wall_clock_sec,
        "segments": segments,
        **stats,
    }


def run_backend_with_engine(engine, audio_path, output_dir, offset_sec=0.0):
    payload = transcribe_one(engine, Path(audio_path), offset_sec)
    output_dir.mkdir(parents=True, exist_ok=True)
    segments_path = output_dir / "transcription-segments.json"
    report_path = output_dir / "parakeet-report.json"
    segments_path.write_text(
        json.dumps(payload["segments"], ensure_ascii=False, indent=2),
        encoding="utf-8",
    )
    report = {
        **payload,
        "segmentCount": len(payload["segments"]),
        "outputFiles": [str(report_path), str(segments_path)],
    }
    report_path.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    return report


def run_backend_batch_with_engine(engine, chunks, output_dir):
    started = time.perf_counter()
    chunk_outputs = []
    audio_duration_sec = 0.0
    window_count = 0

    for fallback_index, chunk in enumerate(chunks):
        audio_path = chunk_value(chunk, "audioPath", "audio_path")
        if not audio_path:
            raise RuntimeError(f"Chunk {fallback_index} is missing audioPath")
        index = int(chunk_value(chunk, "index", default=fallback_index))
        offset_sec = float(chunk_value(chunk, "offsetSec", "offset_sec", default=0.0) or 0.0)
        duration_sec = float(chunk_value(chunk, "durationSec", "duration_sec", default=0.0) or 0.0)
        payload = transcribe_one(engine, Path(audio_path), offset_sec)
        chunk_outputs.append(
            {
                "index": index,
                "audioPath": audio_path,
                "offsetSec": offset_sec,
                "durationSec": duration_sec,
                "segments": payload["segments"],
                "report": payload,
            }
        )
        audio_duration_sec += duration_sec or payload.get("audioDurationSec", 0.0)
        window_count += int(payload.get("windowCount", 0) or 0)

    wall_clock_sec = time.perf_counter() - started
    output_dir.mkdir(parents=True, exist_ok=True)
    chunks_path = output_dir / "chunk-transcription-results.json"
    report_path = output_dir / "parakeet-batch-report.json"
    chunks_path.write_text(
        json.dumps(chunk_outputs, ensure_ascii=False, indent=2),
        encoding="utf-8",
    )
    segment_count = sum(len(chunk["segments"]) for chunk in chunk_outputs)
    report = {
        "backend": "parakeet-tdt-windowed",
        "model": engine.model,
        "device": engine.device,
        "batchSize": engine.batch_size,
        "windowSec": engine.window_sec,
        "overlapSec": engine.overlap_sec,
        "chunkCount": len(chunk_outputs),
        "windowCount": window_count,
        "audioDurationSec": audio_duration_sec,
        "wallClockSec": wall_clock_sec,
        "segmentCount": segment_count,
        "realtimeFactor": wall_clock_sec / audio_duration_sec if audio_duration_sec > 0 else None,
        "speedX": audio_duration_sec / wall_clock_sec if wall_clock_sec > 0 else None,
        "outputFiles": [str(report_path), str(chunks_path)],
    }
    report_path.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    return report


def make_engine(args):
    return ParakeetEngine(
        model=args.model,
        device=args.device,
        batch_size=args.batch_size,
        window_sec=args.window_sec,
        overlap_sec=args.overlap_sec,
    )


def run_backend(args):
    engine = make_engine(args)
    output_dir = Path(args.out_dir)
    if args.chunks_json:
        chunks = json.loads(Path(args.chunks_json).read_text(encoding="utf-8"))
        if not isinstance(chunks, list):
            raise RuntimeError("--chunks-json must contain a JSON array")
        return run_backend_batch_with_engine(engine, chunks, output_dir)
    if not args.audio:
        raise SystemExit("--audio is required unless --chunks-json is used")
    return run_backend_with_engine(engine, Path(args.audio), output_dir, offset_sec=args.offset_sec)


def compact_report(report):
    return {
        "backend": report["backend"],
        "model": report["model"],
        "device": report["device"],
        "batchSize": report["batchSize"],
        "windowSec": report["windowSec"],
        "overlapSec": report["overlapSec"],
        "chunkCount": report.get("chunkCount"),
        "windowCount": report.get("windowCount"),
        "audioDurationSec": report.get("audioDurationSec"),
        "wallClockSec": report["wallClockSec"],
        "realtimeFactor": report.get("realtimeFactor"),
        "speedX": report.get("speedX"),
        "segmentCount": report.get("segmentCount"),
        "outputFiles": report["outputFiles"],
    }


def parse_args():
    parser = argparse.ArgumentParser(description="Run local Parakeet TDT transcription backend.")
    parser.add_argument("--audio", help="Audio file path for single-file mode")
    parser.add_argument("--chunks-json", help="JSON file with chunk metadata for batch mode")
    parser.add_argument("--out-dir", required=True, help="Output directory")
    parser.add_argument("--offset-sec", type=float, default=0.0)
    parser.add_argument("--model", default=DEFAULT_MODEL)
    parser.add_argument("--device", default="auto", choices=["auto", "cpu", "cuda"])
    parser.add_argument("--batch-size", type=int, default=4)
    parser.add_argument("--window-sec", type=float, default=30.0)
    parser.add_argument("--overlap-sec", type=float, default=1.0)
    parser.add_argument("--json", action="store_true", help="Print the full JSON report")
    return parser.parse_args()


if __name__ == "__main__":
    parsed_args = parse_args()
    result = run_backend(parsed_args)
    if parsed_args.json:
        print(json.dumps(result, ensure_ascii=False))
    else:
        print(json.dumps(compact_report(result), ensure_ascii=False))
