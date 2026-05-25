import argparse
import json
import os
import time
from pathlib import Path


MODEL_ALIASES = {
    "whisper-large-v3-turbo": "turbo",
    "large-v3-turbo": "turbo",
    "faster-whisper-large-v3-turbo": "turbo",
}


def normalize_model_name(value):
    raw = str(value or "turbo").strip()
    return MODEL_ALIASES.get(raw.lower(), raw or "turbo")


def resolve_model_name(value):
    normalized = normalize_model_name(value)
    bundled_model_dir = os.environ.get("MEETING_MINUTES_FAST_WHISPER_MODEL_DIR", "").strip()
    if normalized == "turbo" and bundled_model_dir and Path(bundled_model_dir).exists():
        return bundled_model_dir
    return normalized


def resolve_device(value):
    raw = str(value or "auto").strip().lower()
    if raw in {"cpu", "cuda"}:
        return raw
    try:
        import ctranslate2

        return "cuda" if ctranslate2.get_cuda_device_count() > 0 else "cpu"
    except Exception:
        return "cpu"


def resolve_compute_type(value, device):
    raw = str(value or "auto").strip().lower()
    if raw and raw != "auto":
        return raw
    return "float16" if device == "cuda" else "int8"


def positive_int(value, fallback, minimum=1, maximum=None):
    try:
        parsed = int(value)
    except (TypeError, ValueError):
        return fallback
    parsed = max(minimum, parsed)
    if maximum is not None:
        parsed = min(maximum, parsed)
    return parsed


def segment_to_dict(segment, offset_sec, index):
    text = str(getattr(segment, "text", "") or "").strip()
    if not text:
        return None
    start = float(getattr(segment, "start", 0.0) or 0.0) + offset_sec
    end = float(getattr(segment, "end", 0.0) or 0.0) + offset_sec
    if end < start:
        end = start
    return {
        "id": index,
        "start": start,
        "end": end,
        "text": text,
    }


def build_payload(
    segments,
    info,
    audio_path,
    offset_sec,
    model,
    device,
    compute_type,
    wall_clock_sec,
):
    normalized_segments = []
    for segment in segments:
        item = segment_to_dict(segment, offset_sec, len(normalized_segments))
        if item is not None:
            normalized_segments.append(item)

    return {
        "backend": "faster-whisper",
        "model": model,
        "device": device,
        "computeType": compute_type,
        "audioPath": str(audio_path),
        "offsetSec": float(offset_sec),
        "wallClockSec": wall_clock_sec,
        "language": str(getattr(info, "language", "") or ""),
        "languageProbability": float(getattr(info, "language_probability", 0.0) or 0.0),
        "segments": normalized_segments,
    }


class FasterWhisperEngine:
    def __init__(
        self,
        model="turbo",
        device="auto",
        compute_type="auto",
        cpu_threads=0,
        num_workers=1,
        batch_size=8,
        beam_size=1,
        language="pt",
        vad_filter=True,
    ):
        self.model = resolve_model_name(model)
        self.device = resolve_device(device)
        self.compute_type = resolve_compute_type(compute_type, self.device)
        self.cpu_threads = positive_int(cpu_threads, max(1, (os.cpu_count() or 4) // 2))
        self.num_workers = positive_int(num_workers, 1)
        self.batch_size = positive_int(batch_size, 8)
        self.beam_size = positive_int(beam_size, 1)
        self.language = str(language or "pt").strip() or "pt"
        self.vad_filter = bool(vad_filter)
        self._model = None
        self._batched_model = None

    @property
    def transcriber(self):
        if self._model is None:
            try:
                from faster_whisper import BatchedInferencePipeline, WhisperModel
            except Exception as exc:
                raise RuntimeError(
                    "Python package 'faster-whisper' is not installed. "
                    "Install it with: npm run setup:transcribe-local"
                ) from exc

            self._model = WhisperModel(
                self.model,
                device=self.device,
                compute_type=self.compute_type,
                cpu_threads=self.cpu_threads,
                num_workers=self.num_workers,
            )
            if self.batch_size > 1:
                self._batched_model = BatchedInferencePipeline(model=self._model)
        return self._batched_model or self._model

    def transcribe(self, audio_path):
        kwargs = {
            "language": self.language,
            "beam_size": self.beam_size,
            "vad_filter": self.vad_filter,
            "condition_on_previous_text": False,
            "temperature": 0.0,
        }
        if self.vad_filter:
            kwargs["vad_parameters"] = {"min_silence_duration_ms": 500}
        if self._batched_model is not None or self.batch_size > 1:
            kwargs["batch_size"] = self.batch_size

        segments, info = self.transcriber.transcribe(str(audio_path), **kwargs)
        return list(segments), info


def chunk_value(chunk, *names, default=None):
    for name in names:
        if name in chunk:
            return chunk[name]
    return default


def transcribe_one(engine, audio_path, offset_sec):
    started = time.perf_counter()
    segments, info = engine.transcribe(str(audio_path))
    wall_clock_sec = time.perf_counter() - started
    return build_payload(
        segments,
        info,
        audio_path,
        offset_sec=offset_sec,
        model=engine.model,
        device=engine.device,
        compute_type=engine.compute_type,
        wall_clock_sec=wall_clock_sec,
    )


def run_backend_with_engine(engine, audio_path, output_dir, offset_sec=0.0):
    payload = transcribe_one(engine, audio_path, offset_sec)
    output_dir.mkdir(parents=True, exist_ok=True)
    segments_path = output_dir / "transcription-segments.json"
    report_path = output_dir / "faster-whisper-report.json"
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
    for fallback_index, chunk in enumerate(chunks):
        audio_path = chunk_value(chunk, "audioPath", "audio_path")
        if not audio_path:
            raise RuntimeError(f"Chunk {fallback_index} is missing audioPath")
        index = int(chunk_value(chunk, "index", default=fallback_index))
        offset_sec = float(chunk_value(chunk, "offsetSec", "offset_sec", default=0.0) or 0.0)
        duration_sec = float(chunk_value(chunk, "durationSec", "duration_sec", default=0.0) or 0.0)
        payload = transcribe_one(engine, audio_path, offset_sec)
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
        audio_duration_sec += duration_sec

    wall_clock_sec = time.perf_counter() - started
    output_dir.mkdir(parents=True, exist_ok=True)
    chunks_path = output_dir / "chunk-transcription-results.json"
    report_path = output_dir / "faster-whisper-batch-report.json"
    chunks_path.write_text(
        json.dumps(chunk_outputs, ensure_ascii=False, indent=2),
        encoding="utf-8",
    )
    segment_count = sum(len(chunk["segments"]) for chunk in chunk_outputs)
    report = {
        "backend": "faster-whisper",
        "model": engine.model,
        "device": engine.device,
        "computeType": engine.compute_type,
        "chunkCount": len(chunk_outputs),
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
    return FasterWhisperEngine(
        model=args.model,
        device=args.device,
        compute_type=args.compute_type,
        cpu_threads=args.cpu_threads,
        num_workers=args.num_workers,
        batch_size=args.batch_size,
        beam_size=args.beam_size,
        language=args.language,
        vad_filter=not args.no_vad,
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
        "computeType": report["computeType"],
        "chunkCount": report.get("chunkCount"),
        "audioDurationSec": report.get("audioDurationSec"),
        "wallClockSec": report["wallClockSec"],
        "realtimeFactor": report.get("realtimeFactor"),
        "speedX": report.get("speedX"),
        "segmentCount": report.get("segmentCount"),
        "outputFiles": report["outputFiles"],
    }


def parse_args():
    parser = argparse.ArgumentParser(description="Run local faster-whisper transcription backend.")
    parser.add_argument("--audio", help="Audio file path for single-file mode")
    parser.add_argument("--chunks-json", help="JSON file with chunk metadata for batch mode")
    parser.add_argument("--out-dir", required=True, help="Output directory")
    parser.add_argument("--offset-sec", type=float, default=0.0)
    parser.add_argument("--model", default="turbo", help="faster-whisper model, e.g. turbo or large-v3")
    parser.add_argument("--device", default="auto", choices=["auto", "cpu", "cuda"])
    parser.add_argument("--compute-type", default="auto")
    parser.add_argument("--cpu-threads", type=int, default=0)
    parser.add_argument("--num-workers", type=int, default=1)
    parser.add_argument("--batch-size", type=int, default=8)
    parser.add_argument("--beam-size", type=int, default=1)
    parser.add_argument("--language", default="pt")
    parser.add_argument("--no-vad", action="store_true")
    parser.add_argument("--json", action="store_true", help="Print the full JSON report")
    return parser.parse_args()


if __name__ == "__main__":
    parsed_args = parse_args()
    result = run_backend(parsed_args)
    if parsed_args.json:
        print(json.dumps(result, ensure_ascii=False))
    else:
        print(json.dumps(compact_report(result), ensure_ascii=False))
