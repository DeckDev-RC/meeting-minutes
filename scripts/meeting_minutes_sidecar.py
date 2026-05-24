import argparse
import concurrent.futures
import importlib
import importlib.metadata
import json
import platform
import re
import sys
import tempfile
import threading
import time
import traceback
from pathlib import Path


SIDECAR_VERSION = "0.1.0"
COMMANDS = ["health", "transcribe-local", "diarize-modern-cpu", "serve"]


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


def transcribe_engine_key(request):
    return (
        request.get("model", "turbo"),
        request.get("device", "auto"),
        request.get("computeType") or request.get("compute_type") or "auto",
        optional_int(request.get("cpuThreads") or request.get("cpu_threads"), 0),
        optional_int(request.get("numWorkers") or request.get("num_workers"), 1),
        optional_int(request.get("batchSize") or request.get("batch_size"), 8),
        optional_int(request.get("beamSize") or request.get("beam_size"), 1),
        request.get("language", "pt"),
        optional_bool(request.get("vadFilter", request.get("vad_filter", True)), True),
    )


def create_transcribe_engine(module, request):
    return module.FasterWhisperEngine(
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


def run_transcribe_with_engine(module, engine, request, response_path=None):
    output_dir = default_output_dir("transcribe-local", request, response_path)
    chunks = request.get("chunks")
    if chunks:
        if not isinstance(chunks, list):
            raise RuntimeError("transcribe-local request field 'chunks' must be a JSON array")
        return module.run_backend_batch_with_engine(engine, chunks, output_dir)

    audio_path = request.get("audioPath") or request.get("audio_path")
    if not audio_path:
        raise RuntimeError("transcribe-local request requires audioPath when chunks is empty")
    return module.run_backend_with_engine(
        engine,
        Path(audio_path),
        output_dir,
        offset_sec=optional_float(request.get("offsetSec") or request.get("offset_sec"), 0.0),
    )


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
    engine = create_transcribe_engine(module, request)
    report = run_transcribe_with_engine(module, engine, request, response_path=response_path)
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


def diarization_request_options(request):
    return {
        "expected_speakers": optional_int(
            request.get("expectedSpeakers") or request.get("expected_speakers")
        ),
        "num_threads": optional_int(request.get("numThreads") or request.get("num_threads"), 1),
        "min_speakers": optional_int(request.get("minSpeakers") or request.get("min_speakers")),
        "max_speakers": optional_int(request.get("maxSpeakers") or request.get("max_speakers")),
        "embedding_profile": request.get("embeddingProfile") or request.get("embedding_profile"),
    }


def normalize_diarize_worker_count(module, requested_workers, chunk_count):
    if hasattr(module, "normalize_max_workers"):
        return module.normalize_max_workers(requested_workers, chunk_count)
    if chunk_count <= 0:
        return 1
    try:
        parsed = int(requested_workers)
    except (TypeError, ValueError):
        return 1
    return max(1, min(parsed, chunk_count))


def build_diarize_batch_report(module, chunk_outputs, output_dir, safe_workers, wall_clock_sec):
    output_dir.mkdir(parents=True, exist_ok=True)
    chunks_path = output_dir / "chunk-diarized-results.json"
    report_path = output_dir / "diarize-batch-backend-report.json"
    chunks_path.write_text(json.dumps(chunk_outputs, ensure_ascii=False, indent=2), encoding="utf-8")

    audio_duration_sec = sum(
        float(chunk["report"].get("audioDurationSec") or 0.0)
        for chunk in chunk_outputs
    )
    speakers = []
    segment_count = 0
    for chunk in chunk_outputs:
        for speaker in chunk["diarized"]["speakers"]:
            if speaker not in speakers:
                speakers.append(speaker)
        segment_count += len(chunk["diarized"]["segments"])

    report = {
        "backend": "diarize",
        "model": "diarize-0.1.2",
        "chunkCount": len(chunk_outputs),
        "maxWorkers": safe_workers,
        "audioDurationSec": audio_duration_sec,
        "wallClockSec": wall_clock_sec,
        "speakerCount": len(speakers) or 1,
        "segmentCount": segment_count,
        "realtimeFactor": wall_clock_sec / audio_duration_sec if audio_duration_sec > 0 else None,
        "speedX": audio_duration_sec / wall_clock_sec if wall_clock_sec > 0 else None,
        "outputFiles": [str(report_path), str(chunks_path)],
    }

    chunk_profiles = [
        chunk["report"].get("profile")
        for chunk in chunk_outputs
        if chunk["report"].get("profile")
    ]
    if chunk_profiles:
        timing_keys = sorted(
            {
                key
                for profile in chunk_profiles
                for key in profile.get("timings", {}).keys()
            }
        )
        report["profile"] = {
            "embeddingProfile": chunk_profiles[0].get("embeddingProfile"),
            "embeddingCount": sum(int(profile.get("embeddingCount") or 0) for profile in chunk_profiles),
            "subsegmentCount": sum(int(profile.get("subsegmentCount") or 0) for profile in chunk_profiles),
            "speechSegmentCount": sum(
                int(profile.get("speechSegmentCount") or 0)
                for profile in chunk_profiles
            ),
            "boundaryRefinementWindowCount": sum(
                int(profile.get("boundaryRefinementWindowCount") or 0)
                for profile in chunk_profiles
            ),
            "timings": {
                key: round(
                    sum(
                        float(profile.get("timings", {}).get(key) or 0.0)
                        for profile in chunk_profiles
                    ),
                    6,
                )
                for key in timing_keys
            },
        }

    warnings_fn = getattr(module, "dependency_warnings", None)
    if warnings_fn is not None:
        warnings = warnings_fn()
        if warnings:
            report["dependencyWarnings"] = warnings

    report_path.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    return report


class PersistentDiarizeWorkerPool:
    def __init__(self, module, worker_count):
        self.module = module
        self.worker_count = worker_count
        self.worker_state = threading.local()
        self.executor = concurrent.futures.ThreadPoolExecutor(
            max_workers=worker_count,
            initializer=self._initialize_worker,
        )
        self._warm_workers()

    def _initialize_worker(self):
        self.worker_state.diarize_fn = self.module.load_diarize_function()

    def _worker_diarize_fn(self):
        if not hasattr(self.worker_state, "diarize_fn"):
            self._initialize_worker()
        return self.worker_state.diarize_fn

    def _warm_workers(self):
        if self.worker_count <= 1:
            return
        barrier = threading.Barrier(self.worker_count)

        def wait_until_ready():
            self._worker_diarize_fn()
            barrier.wait(timeout=300)

        futures = [
            self.executor.submit(wait_until_ready)
            for _ in range(self.worker_count)
        ]
        for future in futures:
            future.result()

    def process_chunk(self, item, kwargs):
        fallback_index, chunk = item
        return self.module.build_batch_chunk_output(
            self._worker_diarize_fn(),
            chunk,
            fallback_index,
            kwargs,
        )

    def run_batch(self, request, options):
        chunks = request.get("chunks")
        if not isinstance(chunks, list):
            raise RuntimeError("diarize-modern-cpu request field 'chunks' must be a JSON array")

        output_dir = default_output_dir("diarize-modern-cpu", request)
        kwargs = self.module.backend_kwargs(
            options["expected_speakers"],
            options["min_speakers"],
            options["max_speakers"],
            options["embedding_profile"],
        )
        started = time.perf_counter()
        futures = [
            self.executor.submit(self.process_chunk, item, kwargs)
            for item in enumerate(chunks)
        ]
        chunk_outputs = [future.result() for future in futures]
        wall_clock_sec = time.perf_counter() - started
        return build_diarize_batch_report(
            self.module,
            chunk_outputs,
            output_dir,
            self.worker_count,
            wall_clock_sec,
        )

    def shutdown(self):
        self.executor.shutdown(wait=True)


def run_diarize_with_function(
    module,
    diarize_fn,
    request,
    response_path=None,
    diarize_factory=None,
):
    output_dir = default_output_dir("diarize-modern-cpu", request, response_path)
    options = diarization_request_options(request)

    chunks = request.get("chunks")
    if chunks:
        if not isinstance(chunks, list):
            raise RuntimeError("diarize-modern-cpu request field 'chunks' must be a JSON array")
        return module.run_backend_batch_with_diarize(
            diarize_fn,
            chunks,
            output_dir,
            num_speakers=options["expected_speakers"],
            min_speakers=options["min_speakers"],
            max_speakers=options["max_speakers"],
            max_workers=options["num_threads"],
            diarize_factory=diarize_factory,
            embedding_profile=options["embedding_profile"],
        )

    audio_path = request.get("audioPath") or request.get("audio_path")
    if not audio_path:
        raise RuntimeError("diarize-modern-cpu request requires audioPath when chunks is empty")
    return module.run_backend_with_diarize(
        diarize_fn,
        Path(audio_path),
        output_dir,
        num_speakers=options["expected_speakers"],
        min_speakers=options["min_speakers"],
        max_speakers=options["max_speakers"],
        embedding_profile=options["embedding_profile"],
    )


def run_diarize_modern_cpu_request(request, response_path=None, backend_module=None):
    module = backend_module or import_backend_module("diarize_cpu_backend")
    started = time.perf_counter()
    report = run_diarize_with_function(
        module,
        None,
        request,
        response_path=response_path,
        diarize_factory=module.load_diarize_function,
    )
    return build_diarization_response(report, time.perf_counter() - started)


class PersistentSidecarSession:
    def __init__(self, transcribe_module=None, diarize_module=None):
        self.transcribe_module = transcribe_module
        self.diarize_module = diarize_module
        self.transcribe_engine = None
        self.transcribe_engine_key = None
        self.diarize_fn = None
        self.diarize_worker_pool = None
        self.diarize_worker_pool_size = None

    def resolve_transcribe_module(self):
        if self.transcribe_module is None:
            self.transcribe_module = import_backend_module("transcribe_faster_whisper_backend")
        return self.transcribe_module

    def resolve_diarize_module(self):
        if self.diarize_module is None:
            self.diarize_module = import_backend_module("diarize_cpu_backend")
        return self.diarize_module

    def get_transcribe_engine(self, request):
        module = self.resolve_transcribe_module()
        key = transcribe_engine_key(request)
        cache_hit = self.transcribe_engine is not None and self.transcribe_engine_key == key
        if not cache_hit:
            self.transcribe_engine = create_transcribe_engine(module, request)
            self.transcribe_engine_key = key
        return module, self.transcribe_engine, cache_hit

    def get_diarize_function(self):
        module = self.resolve_diarize_module()
        cache_hit = self.diarize_fn is not None
        if self.diarize_fn is None:
            self.diarize_fn = module.load_diarize_function()
        return module, self.diarize_fn, cache_hit

    def get_diarize_worker_pool(self, worker_count):
        module = self.resolve_diarize_module()
        cache_hit = (
            self.diarize_worker_pool is not None
            and self.diarize_worker_pool_size == worker_count
        )
        if not cache_hit:
            self.close_diarize_worker_pool()
            self.diarize_worker_pool = PersistentDiarizeWorkerPool(module, worker_count)
            self.diarize_worker_pool_size = worker_count
        return self.diarize_worker_pool, cache_hit

    def close_diarize_worker_pool(self):
        if self.diarize_worker_pool is not None:
            self.diarize_worker_pool.shutdown()
            self.diarize_worker_pool = None
            self.diarize_worker_pool_size = None

    def close(self):
        self.close_diarize_worker_pool()

    def run_transcribe_local(self, request):
        started = time.perf_counter()
        module, engine, cache_hit = self.get_transcribe_engine(request)
        report = run_transcribe_with_engine(module, engine, request)
        response = build_transcription_response(report, time.perf_counter() - started)
        response["telemetry"]["persistent"] = True
        response["telemetry"]["engineCacheHit"] = cache_hit
        return response

    def run_diarize_modern_cpu(self, request):
        started = time.perf_counter()
        options = diarization_request_options(request)
        module = self.resolve_diarize_module()
        chunks = request.get("chunks")
        safe_workers = (
            normalize_diarize_worker_count(module, options["num_threads"], len(chunks))
            if isinstance(chunks, list)
            else 1
        )
        if safe_workers <= 1:
            module, diarize_fn, cache_hit = self.get_diarize_function()
            report = run_diarize_with_function(module, diarize_fn, request)
            cache_scope = "single-worker"
            pool_cache_hit = None
        else:
            pool, pool_cache_hit = self.get_diarize_worker_pool(safe_workers)
            report = pool.run_batch(request, options)
            cache_hit = pool_cache_hit
            cache_scope = "parallel-worker-pool"
        response = build_diarization_response(report, time.perf_counter() - started)
        response["telemetry"]["persistent"] = True
        response["telemetry"]["diarizeCacheHit"] = cache_hit
        response["telemetry"]["diarizeCacheScope"] = cache_scope
        if pool_cache_hit is not None:
            response["telemetry"]["diarizePoolCacheHit"] = pool_cache_hit
            response["telemetry"]["diarizePoolWorkers"] = safe_workers
        return response

    def handle(self, envelope):
        if not isinstance(envelope, dict):
            raise RuntimeError("Persistent sidecar envelope must be a JSON object")
        command = envelope.get("command")
        request = envelope.get("request") or {}
        if request and not isinstance(request, dict):
            raise RuntimeError("Persistent sidecar envelope field 'request' must be a JSON object")

        if command == "health":
            response = command_health()
        elif command == "transcribe-local":
            response = self.run_transcribe_local(request)
        elif command == "diarize-modern-cpu":
            response = self.run_diarize_modern_cpu(request)
        elif command == "shutdown":
            self.close()
            response = {"ok": True, "command": "shutdown"}
        else:
            raise RuntimeError(f"Unsupported persistent sidecar command: {command}")

        if "id" in envelope:
            response["id"] = envelope["id"]
        return response


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


def persistent_error_response(envelope, exc, started_at):
    command = envelope.get("command") if isinstance(envelope, dict) else None
    response = error_response(command or "serve", exc, started_at)
    if isinstance(envelope, dict) and "id" in envelope:
        response["id"] = envelope["id"]
    return response


def write_json_line(output_stream, payload):
    output_stream.write(json.dumps(payload, ensure_ascii=False) + "\n")
    output_stream.flush()


def run_persistent_server(input_stream=None, output_stream=None, session=None):
    input_stream = input_stream or sys.stdin
    output_stream = output_stream or sys.stdout
    session = session or PersistentSidecarSession()

    try:
        for line in input_stream:
            raw = line.strip()
            if not raw:
                continue
            started = time.perf_counter()
            envelope = None
            try:
                envelope = json.loads(raw)
                response = session.handle(envelope)
            except Exception as exc:
                response = persistent_error_response(envelope or {}, exc, started)
            write_json_line(output_stream, response)
            if isinstance(envelope, dict) and envelope.get("command") == "shutdown":
                return 0
        return 0
    finally:
        session.close()


def parse_args():
    parser = argparse.ArgumentParser(description="Experimental meeting-minutes sidecar.")
    subparsers = parser.add_subparsers(dest="command", required=True)

    health = subparsers.add_parser("health", help="Print sidecar health and dependency status")
    health.add_argument("--output", help="Optional response JSON path")

    subparsers.add_parser(
        "serve",
        help="Run a persistent JSONL sidecar over stdin/stdout",
    )

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
        if args.command == "serve":
            return run_persistent_server()

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
