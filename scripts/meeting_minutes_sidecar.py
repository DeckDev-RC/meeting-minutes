import argparse
import importlib
import importlib.metadata
import json
import platform
import re
import sys
import tempfile
import time
import traceback
from pathlib import Path


SIDECAR_VERSION = "0.1.0"
COMMANDS = ["health", "transcribe-local", "diarize-modern-cpu"]


def load_json_object(path):
    payload = json.loads(Path(path).read_text(encoding="utf-8"))
    if not isinstance(payload, dict):
        raise RuntimeError(f"{path} must contain a JSON object")
    return payload


def write_json(path, payload):
    if path is None:
        print(json.dumps(payload, ensure_ascii=False))
        return
    output_path = Path(path)
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(json.dumps(payload, ensure_ascii=False, indent=2), encoding="utf-8")


def default_output_dir(command, request, response_path=None):
    explicit = request.get("outputDir") or request.get("output_dir")
    if explicit:
        return Path(explicit)
    if response_path:
        return Path(response_path).with_suffix("").with_name(f"{Path(response_path).stem}-artifacts")
    return Path(tempfile.mkdtemp(prefix=f"meeting-minutes-sidecar-{command}-"))


def optional_int(value, fallback=None):
    if value is None or value == "":
        return fallback
    try:
        return int(value)
    except (TypeError, ValueError):
        return fallback


def optional_float(value, fallback=0.0):
    if value is None or value == "":
        return fallback
    try:
        return float(value)
    except (TypeError, ValueError):
        return fallback


def optional_bool(value, fallback=False):
    if isinstance(value, bool):
        return value
    if value is None or value == "":
        return fallback
    raw = str(value).strip().lower()
    if raw in {"1", "true", "yes", "y", "on"}:
        return True
    if raw in {"0", "false", "no", "n", "off"}:
        return False
    return fallback


def package_status(package_name):
    try:
        return {
            "available": True,
            "version": importlib.metadata.version(package_name),
        }
    except importlib.metadata.PackageNotFoundError:
        return {"available": False, "version": None}


def command_health():
    packages = {
        name: package_status(name)
        for name in [
            "faster-whisper",
            "ctranslate2",
            "diarize",
            "silero-vad",
            "soundfile",
            "torch",
            "torchaudio",
            "wespeakerruntime",
        ]
    }
    return {
        "ok": True,
        "command": "health",
        "sidecarVersion": SIDECAR_VERSION,
        "pythonExecutable": sys.executable,
        "pythonVersion": platform.python_version(),
        "platform": platform.platform(),
        "commands": COMMANDS,
        "packages": packages,
        "capabilities": {
            "transcribeLocal": packages["faster-whisper"]["available"],
            "diarizeModernCpu": all(
                packages[name]["available"]
                for name in ["diarize", "silero-vad", "soundfile", "wespeakerruntime"]
            ),
        },
    }


def import_backend_module(name):
    return importlib.import_module(name)


def load_transcription_chunk_outputs(report):
    for output_file in report.get("outputFiles", []):
        path = Path(output_file)
        if path.name == "chunk-transcription-results.json" and path.is_file():
            return json.loads(path.read_text(encoding="utf-8"))
    return []


def transcription_segments_from_report(report, chunk_outputs=None):
    segments = report.get("segments")
    if isinstance(segments, list):
        return segments

    if chunk_outputs is None:
        chunk_outputs = load_transcription_chunk_outputs(report)

    flattened = []
    for chunk in chunk_outputs or []:
        for segment in chunk.get("segments", []):
            item = dict(segment)
            item["id"] = len(flattened)
            flattened.append(item)
    return flattened


def build_transcription_response(report, command_wall_clock_sec, chunk_outputs=None):
    segments = transcription_segments_from_report(report, chunk_outputs=chunk_outputs)
    return {
        "ok": True,
        "command": "transcribe-local",
        "segments": segments,
        "report": report,
        "telemetry": {
            "backend": "sidecar-faster-whisper",
            "model": report.get("model"),
            "device": report.get("device"),
            "computeType": report.get("computeType"),
            "wallClockSec": report.get("wallClockSec", command_wall_clock_sec),
            "commandWallClockSec": command_wall_clock_sec,
            "chunkCount": report.get("chunkCount"),
            "segmentCount": report.get("segmentCount", len(segments)),
            "speedX": report.get("speedX"),
            "realtimeFactor": report.get("realtimeFactor"),
        },
    }


def run_transcribe_local_request(request, response_path=None, backend_module=None):
    module = backend_module or import_backend_module("transcribe_faster_whisper_backend")
    started = time.perf_counter()
    output_dir = default_output_dir("transcribe-local", request, response_path)
    engine = module.FasterWhisperEngine(
        model=request.get("model", "turbo"),
        device=request.get("device", "auto"),
        compute_type=request.get("computeType") or request.get("compute_type") or "auto",
        cpu_threads=optional_int(request.get("cpuThreads") or request.get("cpu_threads"), 0),
        num_workers=optional_int(request.get("numWorkers") or request.get("num_workers"), 1),
        batch_size=optional_int(request.get("batchSize") or request.get("batch_size"), 8),
        beam_size=optional_int(request.get("beamSize") or request.get("beam_size"), 1),
        language=request.get("language", "pt"),
        vad_filter=optional_bool(request.get("vadFilter", request.get("vad_filter", True)), True),
    )

    chunks = request.get("chunks")
    if chunks:
        if not isinstance(chunks, list):
            raise RuntimeError("transcribe-local request field 'chunks' must be a JSON array")
        report = module.run_backend_batch_with_engine(engine, chunks, output_dir)
    else:
        audio_path = request.get("audioPath") or request.get("audio_path")
        if not audio_path:
            raise RuntimeError("transcribe-local request requires audioPath when chunks is empty")
        report = module.run_backend_with_engine(
            engine,
            Path(audio_path),
            output_dir,
            offset_sec=optional_float(request.get("offsetSec") or request.get("offset_sec"), 0.0),
        )

    return build_transcription_response(report, time.perf_counter() - started)


def speaker_label_to_index(value):
    raw = str(value or "").strip()
    if raw.upper().startswith("SPEAKER_"):
        suffix = raw.split("_", 1)[1].lstrip("0") or "0"
        return int(suffix) if suffix.isdigit() else 0
    match = re.search(r"(\d+)$", raw)
    if match:
        return max(0, int(match.group(1)) - 1)
    return 0


def diarized_segments_to_turns(segments, offset_sec=0.0):
    turns = []
    for segment in segments or []:
        start = optional_float(segment.get("start"), 0.0) + offset_sec
        end = optional_float(segment.get("end"), start) + offset_sec
        if end < start:
            end = start
        turns.append(
            {
                "start": start,
                "end": end,
                "speakerIndex": speaker_label_to_index(segment.get("speaker")),
            }
        )
    return turns


def load_chunk_outputs(report):
    for output_file in report.get("outputFiles", []):
        path = Path(output_file)
        if path.name == "chunk-diarized-results.json" and path.is_file():
            return json.loads(path.read_text(encoding="utf-8"))
    return []


def build_diarization_response(report, command_wall_clock_sec, chunk_outputs=None):
    if chunk_outputs is None:
        chunk_outputs = load_chunk_outputs(report)

    if chunk_outputs:
        turns = []
        for chunk in chunk_outputs:
            offset_sec = optional_float(chunk.get("offsetSec") or chunk.get("offset_sec"), 0.0)
            segments = chunk.get("diarized", {}).get("segments", [])
            turns.extend(diarized_segments_to_turns(segments, offset_sec=offset_sec))
    else:
        turns = diarized_segments_to_turns(report.get("segments", []))

    return {
        "ok": True,
        "command": "diarize-modern-cpu",
        "turns": turns,
        "speakers": report.get("speakers"),
        "report": report,
        "telemetry": {
            "backend": "sidecar-modern-cpu",
            "model": report.get("model"),
            "wallClockSec": report.get("wallClockSec", command_wall_clock_sec),
            "commandWallClockSec": command_wall_clock_sec,
            "modelLoadSec": report.get("modelLoadSec"),
            "chunkCount": report.get("chunkCount"),
            "speakerCount": report.get("speakerCount"),
            "segmentCount": report.get("segmentCount"),
            "speedX": report.get("speedX"),
            "realtimeFactor": report.get("realtimeFactor"),
        },
    }


def run_diarize_modern_cpu_request(request, response_path=None, backend_module=None):
    module = backend_module or import_backend_module("diarize_cpu_backend")
    started = time.perf_counter()
    output_dir = default_output_dir("diarize-modern-cpu", request, response_path)
    expected_speakers = optional_int(
        request.get("expectedSpeakers") or request.get("expected_speakers")
    )
    num_threads = optional_int(request.get("numThreads") or request.get("num_threads"), 1)
    min_speakers = optional_int(request.get("minSpeakers") or request.get("min_speakers"))
    max_speakers = optional_int(request.get("maxSpeakers") or request.get("max_speakers"))
    embedding_profile = request.get("embeddingProfile") or request.get("embedding_profile")

    chunks = request.get("chunks")
    if chunks:
        if not isinstance(chunks, list):
            raise RuntimeError("diarize-modern-cpu request field 'chunks' must be a JSON array")
        report = module.run_backend_batch_with_diarize(
            None,
            chunks,
            output_dir,
            num_speakers=expected_speakers,
            min_speakers=min_speakers,
            max_speakers=max_speakers,
            max_workers=num_threads,
            diarize_factory=module.load_diarize_function,
            embedding_profile=embedding_profile,
        )
    else:
        audio_path = request.get("audioPath") or request.get("audio_path")
        if not audio_path:
            raise RuntimeError("diarize-modern-cpu request requires audioPath when chunks is empty")
        report = module.run_backend_with_diarize(
            module.load_diarize_function(),
            Path(audio_path),
            output_dir,
            num_speakers=expected_speakers,
            min_speakers=min_speakers,
            max_speakers=max_speakers,
            embedding_profile=embedding_profile,
        )

    return build_diarization_response(report, time.perf_counter() - started)


def error_response(command, exc, started_at):
    return {
        "ok": False,
        "command": command,
        "error": str(exc),
        "errorType": exc.__class__.__name__,
        "telemetry": {
            "commandWallClockSec": time.perf_counter() - started_at,
        },
        "traceback": traceback.format_exc(),
    }


def parse_args():
    parser = argparse.ArgumentParser(description="Experimental meeting-minutes sidecar.")
    subparsers = parser.add_subparsers(dest="command", required=True)

    health = subparsers.add_parser("health", help="Print sidecar health and dependency status")
    health.add_argument("--output", help="Optional response JSON path")

    for command in ["transcribe-local", "diarize-modern-cpu"]:
        sub = subparsers.add_parser(command, help=f"Run {command} from a JSON request")
        sub.add_argument("--input", required=True, help="Request JSON path")
        sub.add_argument("--output", required=True, help="Response JSON path")

    return parser.parse_args()


def main():
    args = parse_args()
    started = time.perf_counter()
    try:
        if args.command == "health":
            response = command_health()
            write_json(args.output, response)
            return 0

        request = load_json_object(args.input)
        if args.command == "transcribe-local":
            response = run_transcribe_local_request(request, response_path=args.output)
        elif args.command == "diarize-modern-cpu":
            response = run_diarize_modern_cpu_request(request, response_path=args.output)
        else:
            raise RuntimeError(f"Unsupported sidecar command: {args.command}")
        write_json(args.output, response)
        return 0
    except Exception as exc:
        response = error_response(args.command, exc, started)
        write_json(getattr(args, "output", None), response)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
