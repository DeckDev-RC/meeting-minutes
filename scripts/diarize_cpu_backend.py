import json
import argparse
import concurrent.futures
import importlib.metadata
import os
import threading
import time
from dataclasses import dataclass
from pathlib import Path
from types import SimpleNamespace

DEFAULT_EMBEDDING_WINDOW_SEC = 1.2
DEFAULT_EMBEDDING_STEP_SEC = 0.6
DEFAULT_MIN_SEGMENT_DURATION_SEC = 0.4


def normalize_speaker_name(value):
    raw = str(value or "").strip()
    if raw.upper().startswith("SPEAKER_"):
        suffix = raw.split("_", 1)[1].lstrip("0") or "0"
        if suffix.isdigit():
            return f"Falante {int(suffix) + 1}"
    if raw.lower().startswith("speaker_"):
        suffix = raw.split("_", 1)[1].lstrip("0") or "0"
        if suffix.isdigit():
            return f"Falante {int(suffix) + 1}"
    return raw or "Falante 1"


def segment_to_dict(segment):
    return {
        "speaker": normalize_speaker_name(getattr(segment, "speaker", "")),
        "start": float(getattr(segment, "start", 0.0)),
        "end": float(getattr(segment, "end", 0.0)),
        "text": "",
    }


def normalize_embedding(values):
    return [float(value) for value in values]


def speaker_centroids_to_list(value):
    if not value:
        return []

    if isinstance(value, dict):
        items = value.items()
    else:
        items = (
            (item.get("speaker"), item.get("embedding"))
            for item in value
            if isinstance(item, dict)
        )

    centroids = []
    for speaker, embedding in items:
        if embedding is None:
            continue
        normalized = normalize_embedding(embedding)
        if not normalized:
            continue
        centroids.append(
            {
                "speaker": normalize_speaker_name(speaker),
                "embedding": normalized,
            }
        )
    return centroids


def build_payload(result, backend, model, audio_path, wall_clock_sec):
    segments = [segment_to_dict(segment) for segment in getattr(result, "segments", [])]
    speakers = []
    for segment in segments:
        if segment["speaker"] not in speakers:
            speakers.append(segment["speaker"])

    payload = {
        "backend": backend,
        "model": model,
        "audioPath": audio_path,
        "audioDurationSec": float(getattr(result, "audio_duration", 0.0) or 0.0),
        "wallClockSec": wall_clock_sec,
        "speakers": speakers or ["Falante 1"],
        "segments": segments,
    }
    speaker_centroids = speaker_centroids_to_list(getattr(result, "speaker_centroids", None))
    if speaker_centroids:
        payload["speakerCentroids"] = speaker_centroids
    profile = getattr(result, "profile", None)
    if profile:
        payload["profile"] = profile
    return payload


@dataclass(frozen=True)
class EmbeddingProfile:
    name: str
    window_sec: float = DEFAULT_EMBEDDING_WINDOW_SEC
    base_step_sec: float = DEFAULT_EMBEDDING_STEP_SEC
    long_step_sec: float = DEFAULT_EMBEDDING_STEP_SEC
    long_segment_threshold_sec: float = 12.0
    boundary_refinement: bool = False
    min_segment_duration_sec: float = DEFAULT_MIN_SEGMENT_DURATION_SEC

    def step_for_duration(self, duration_sec):
        if duration_sec >= self.long_segment_threshold_sec:
            return self.long_step_sec
        return self.base_step_sec


def embedding_profile_config(value=None):
    raw = (
        value
        or os.environ.get("MEETING_MINUTES_DIARIZE_EMBEDDING_PROFILE")
        or "balanced"
    )
    name = str(raw).strip().lower()
    if name in {"quality", "precise", "dense"}:
        return EmbeddingProfile(name="quality", boundary_refinement=False)
    if name in {"fast", "coarse"}:
        return EmbeddingProfile(
            name="fast",
            long_step_sec=1.5,
            long_segment_threshold_sec=2.0,
            boundary_refinement=False,
        )
    if name in {"boundary", "balanced-boundary"}:
        return EmbeddingProfile(
            name="boundary",
            long_step_sec=1.2,
            long_segment_threshold_sec=2.0,
            boundary_refinement=True,
        )
    return EmbeddingProfile(
        name="balanced",
        long_step_sec=1.2,
        long_segment_threshold_sec=2.0,
        boundary_refinement=False,
    )


def segment_duration(segment):
    duration = getattr(segment, "duration", None)
    if duration is not None:
        return float(duration)
    return max(0.0, float(getattr(segment, "end", 0.0)) - float(getattr(segment, "start", 0.0)))


def build_embedding_windows(segment, profile):
    duration = segment_duration(segment)
    start = float(getattr(segment, "start", 0.0))
    end = float(getattr(segment, "end", start))
    if duration <= profile.window_sec * 1.5:
        return [(start, end)]

    step_sec = profile.step_for_duration(duration)
    windows = []
    window_start = start
    while window_start + profile.min_segment_duration_sec < end:
        window_end = min(window_start + profile.window_sec, end)
        windows.append((round(window_start, 6), round(window_end, 6)))
        window_start += step_sec
    return windows


def backend_kwargs(
    num_speakers=None,
    min_speakers=None,
    max_speakers=None,
    embedding_profile=None,
):
    kwargs = {}
    if num_speakers is not None:
        kwargs["num_speakers"] = num_speakers
    if min_speakers is not None:
        kwargs["min_speakers"] = min_speakers
    if max_speakers is not None:
        kwargs["max_speakers"] = max_speakers
    if embedding_profile is not None:
        kwargs["embedding_profile"] = embedding_profile
    return kwargs


def parse_major_minor(version):
    parts = []
    for raw in str(version).split(".")[:2]:
        digits = "".join(ch for ch in raw if ch.isdigit())
        if not digits:
            break
        parts.append(int(digits))
    return tuple(parts) if len(parts) == 2 else None


def dependency_warnings(versions=None):
    if versions is None:
        versions = {}
        for package in ["torch", "torchaudio", "diarize", "silero-vad", "wespeakerruntime"]:
            try:
                versions[package] = importlib.metadata.version(package)
            except importlib.metadata.PackageNotFoundError:
                continue

    warnings = []
    torchaudio_version = versions.get("torchaudio")
    if torchaudio_version:
        parsed = parse_major_minor(torchaudio_version)
        if parsed is not None and parsed >= (2, 9):
            warnings.append(
                "torchaudio>=2.9 may remove sox_effects used by silero-vad; "
                "pin torch/torchaudio to 2.8.x or migrate audio loading."
            )
    return warnings


def _normalize_rows(values):
    import numpy as np

    values = np.asarray(values, dtype=float)
    norms = np.linalg.norm(values, axis=1, keepdims=True)
    return np.divide(values, norms, out=np.zeros_like(values), where=norms > 0)


def build_speaker_centroids(embeddings, labels):
    import numpy as np

    if len(embeddings) == 0 or len(embeddings) != len(labels):
        return {}

    normalized = _normalize_rows(embeddings)
    centroids = {}
    for label in sorted({int(label) for label in labels}):
        members = normalized[labels == label]
        if len(members) == 0:
            continue
        centroid = _normalize_rows(members.mean(axis=0, keepdims=True))[0]
        if np.any(centroid):
            centroids[f"SPEAKER_{label:02d}"] = centroid.tolist()
    return centroids


def _speaker_centroid_matrix(embeddings, labels):
    import numpy as np

    if len(embeddings) == 0 or len(embeddings) != len(labels):
        return [], np.empty((0, 0), dtype=float)

    normalized = _normalize_rows(embeddings)
    label_values = sorted({int(label) for label in labels})
    centroids = []
    for label in label_values:
        members = normalized[labels == label]
        if len(members) == 0:
            continue
        centroid = _normalize_rows(members.mean(axis=0, keepdims=True))[0]
        if np.any(centroid):
            centroids.append(centroid)
    if not centroids:
        return [], np.empty((0, 0), dtype=float)
    return label_values, np.stack(centroids)


def assign_embeddings_to_centroids(embeddings, base_embeddings, labels):
    import numpy as np

    label_values, centroids = _speaker_centroid_matrix(base_embeddings, labels)
    if not label_values or len(embeddings) == 0:
        return np.empty((0,), dtype=int)

    scores = _normalize_rows(embeddings) @ centroids.T
    best = scores.argmax(axis=1)
    return np.asarray([label_values[index] for index in best], dtype=int)


class ReusableDiarizeEngine:
    def __init__(self, speaker_factory=None):
        self._speaker_factory = speaker_factory
        self._speaker = None
        self._vad_model = None

    @property
    def speaker(self):
        if self._speaker is None:
            if self._speaker_factory is not None:
                self._speaker = self._speaker_factory()
            else:
                import wespeakerruntime as wespeaker_rt

                self._speaker = wespeaker_rt.Speaker(lang="en")
        return self._speaker

    @property
    def vad_model(self):
        if self._vad_model is None:
            from silero_vad import load_silero_vad

            self._vad_model = load_silero_vad()
        return self._vad_model

    def __call__(self, audio_path, **kwargs):
        return self.diarize(audio_path, **kwargs)

    def read_audio_for_vad(self, audio_path, sampling_rate=16000):
        import numpy as np
        import soundfile as sf
        import torch

        audio_data, sample_rate = sf.read(str(audio_path), dtype="float32")
        if audio_data.ndim > 1:
            audio_data = audio_data.mean(axis=1)

        wav = torch.from_numpy(np.ascontiguousarray(audio_data))
        if sample_rate != sampling_rate:
            import torchaudio

            wav = torchaudio.transforms.Resample(sample_rate, sampling_rate)(wav)
        return wav

    def read_audio_data(self, audio_path):
        import numpy as np
        import soundfile as sf

        audio_data, sample_rate = sf.read(str(audio_path), dtype="float32")
        if audio_data.ndim > 1:
            audio_data = audio_data.mean(axis=1)
        return np.ascontiguousarray(audio_data, dtype=np.float32), sample_rate

    def run_vad(self, audio_path):
        from diarize.utils import SpeechSegment
        from silero_vad import get_speech_timestamps

        wav = self.read_audio_for_vad(audio_path)
        speech_timestamps = get_speech_timestamps(
            wav,
            self.vad_model,
            sampling_rate=16000,
            threshold=0.45,
            min_speech_duration_ms=200,
            min_silence_duration_ms=50,
            speech_pad_ms=20,
            return_seconds=True,
        )
        return [SpeechSegment(start=ts["start"], end=ts["end"]) for ts in speech_timestamps]

    def extract_embedding_from_audio(self, segment_audio, sample_rate):
        import numpy as np
        import torch
        import torchaudio
        import torchaudio.compliance.kaldi as kaldi

        if len(segment_audio) == 0:
            return None

        waveform = torch.from_numpy(np.ascontiguousarray(segment_audio, dtype=np.float32)).unsqueeze(0)
        if sample_rate != 16000:
            waveform = torchaudio.transforms.Resample(sample_rate, 16000)(waveform)
            sample_rate = 16000
        waveform = waveform * (1 << 15)
        features = kaldi.fbank(
            waveform,
            num_mel_bins=80,
            frame_length=25,
            frame_shift=10,
            dither=0.0,
            sample_frequency=sample_rate,
            window_type="hamming",
            use_energy=False,
        ).numpy()
        if features.size == 0:
            return None
        features = features - np.mean(features, axis=0)
        features = np.expand_dims(features, 0)
        return self.speaker.extract_embedding_feat(features)

    def extract_embeddings_from_audio(self, audio_data, sample_rate, speech_segments, profile):
        import numpy as np
        from diarize.utils import SubSegment

        embeddings = []
        subsegments = []
        stats = {
            "windowCount": 0,
            "adaptiveWindowCount": 0,
            "shortSegmentWindowCount": 0,
            "skippedShortSpeechSegments": 0,
        }

        for index, segment in enumerate(speech_segments):
            duration = segment_duration(segment)
            if duration < profile.min_segment_duration_sec:
                stats["skippedShortSpeechSegments"] += 1
                continue

            windows = build_embedding_windows(segment, profile)
            stats["windowCount"] += len(windows)
            if duration <= profile.window_sec * 1.5:
                stats["shortSegmentWindowCount"] += len(windows)
            elif profile.step_for_duration(duration) > profile.base_step_sec:
                stats["adaptiveWindowCount"] += len(windows)

            for window_start, window_end in windows:
                start_sample = int(window_start * sample_rate)
                end_sample = int(window_end * sample_rate)
                segment_audio = audio_data[start_sample:end_sample]

                try:
                    embedding = self.extract_embedding_from_audio(segment_audio, sample_rate)
                except Exception:
                    continue

                if embedding is not None:
                    embedding = np.asarray(embedding)
                    if embedding.ndim == 2:
                        embedding = embedding[0]
                    embeddings.append(embedding)
                    subsegments.append(
                        SubSegment(start=window_start, end=window_end, parent_idx=index)
                    )

        if not embeddings:
            return np.empty((0, 256), dtype=np.float32), [], stats
        return np.stack(embeddings), subsegments, stats

    def extract_boundary_embeddings_from_audio(
        self,
        audio_data,
        sample_rate,
        speech_segments,
        subsegments,
        labels,
        profile,
    ):
        import numpy as np
        from diarize.utils import SubSegment

        if not profile.boundary_refinement or len(subsegments) < 2:
            return np.empty((0, 256), dtype=np.float32), []

        subsegments_by_parent = {}
        for index, subsegment in enumerate(subsegments):
            subsegments_by_parent.setdefault(subsegment.parent_idx, []).append(index)

        embeddings = []
        refined_subsegments = []
        seen = set()
        for parent_idx, indices in subsegments_by_parent.items():
            if len(indices) < 2:
                continue
            indices.sort(key=lambda idx: subsegments[idx].start)
            speech_segment = speech_segments[parent_idx]
            for left_idx, right_idx in zip(indices, indices[1:]):
                if int(labels[left_idx]) == int(labels[right_idx]):
                    continue

                left = subsegments[left_idx]
                right = subsegments[right_idx]
                left_center = (left.start + left.end) / 2
                right_center = (right.start + right.end) / 2
                center = (left_center + right_center) / 2
                window_start = max(speech_segment.start, center - profile.window_sec / 2)
                window_end = min(speech_segment.end, window_start + profile.window_sec)
                if window_end - window_start < profile.min_segment_duration_sec:
                    continue
                key = (parent_idx, round(window_start, 2), round(window_end, 2))
                if key in seen:
                    continue
                seen.add(key)

                start_sample = int(window_start * sample_rate)
                end_sample = int(window_end * sample_rate)
                segment_audio = audio_data[start_sample:end_sample]
                try:
                    embedding = self.extract_embedding_from_audio(segment_audio, sample_rate)
                except Exception:
                    continue
                if embedding is None:
                    continue
                embedding = np.asarray(embedding)
                if embedding.ndim == 2:
                    embedding = embedding[0]
                embeddings.append(embedding)
                refined_subsegments.append(
                    SubSegment(start=window_start, end=window_end, parent_idx=parent_idx)
                )

        if not embeddings:
            return np.empty((0, 256), dtype=np.float32), []
        return np.stack(embeddings), refined_subsegments

    def sort_labeled_embeddings(self, embeddings, subsegments, labels):
        import numpy as np

        order = sorted(
            range(len(subsegments)),
            key=lambda idx: (subsegments[idx].parent_idx, subsegments[idx].start, subsegments[idx].end),
        )
        if not order:
            return embeddings, subsegments, labels
        return (
            embeddings[order],
            [subsegments[idx] for idx in order],
            np.asarray([labels[idx] for idx in order], dtype=int),
        )

    def diarize(
        self,
        audio_path,
        *,
        min_speakers=1,
        max_speakers=20,
        num_speakers=None,
        embedding_profile=None,
    ):
        from diarize import _build_diarization_segments
        from diarize.clustering import cluster_speakers
        from diarize.utils import get_audio_duration

        if min_speakers < 1:
            raise ValueError(f"min_speakers must be >= 1, got {min_speakers}")
        if max_speakers < min_speakers:
            raise ValueError(
                f"max_speakers ({max_speakers}) must be >= min_speakers ({min_speakers})"
            )
        if num_speakers is not None and num_speakers < 1:
            raise ValueError(f"num_speakers must be >= 1, got {num_speakers}")

        audio_path = str(audio_path)
        profile = embedding_profile_config(embedding_profile)
        timings = {}
        probe_started = time.perf_counter()
        duration = get_audio_duration(audio_path)
        timings["audioProbeSec"] = time.perf_counter() - probe_started
        vad_started = time.perf_counter()
        speech_segments = self.run_vad(audio_path)
        timings["vadSec"] = time.perf_counter() - vad_started
        if not speech_segments:
            return SimpleNamespace(
                audio_duration=duration,
                segments=[],
                speaker_centroids={},
                profile=self.build_profile(profile, timings, len(speech_segments), 0, 0, {}),
            )

        embedding_started = time.perf_counter()
        audio_data, sample_rate = self.read_audio_data(audio_path)
        embeddings, subsegments, embedding_stats = self.extract_embeddings_from_audio(
            audio_data,
            sample_rate,
            speech_segments,
            profile,
        )
        timings["embeddingSec"] = time.perf_counter() - embedding_started
        if len(embeddings) == 0:
            return SimpleNamespace(
                audio_duration=duration,
                segments=[],
                speaker_centroids={},
                profile=self.build_profile(
                    profile,
                    timings,
                    len(speech_segments),
                    len(subsegments),
                    len(embeddings),
                    embedding_stats,
                ),
            )

        clustering_started = time.perf_counter()
        labels, _estimation_details = cluster_speakers(
            embeddings,
            min_speakers=min_speakers,
            max_speakers=max_speakers,
            num_speakers=num_speakers,
        )
        timings["clusteringSec"] = time.perf_counter() - clustering_started

        boundary_started = time.perf_counter()
        boundary_embeddings, boundary_subsegments = self.extract_boundary_embeddings_from_audio(
            audio_data,
            sample_rate,
            speech_segments,
            subsegments,
            labels,
            profile,
        )
        boundary_labels = assign_embeddings_to_centroids(boundary_embeddings, embeddings, labels)
        if len(boundary_embeddings) > 0 and len(boundary_labels) == len(boundary_embeddings):
            import numpy as np

            embeddings = np.concatenate([embeddings, boundary_embeddings], axis=0)
            labels = np.concatenate([labels, boundary_labels], axis=0)
            subsegments = subsegments + boundary_subsegments
            embeddings, subsegments, labels = self.sort_labeled_embeddings(
                embeddings,
                subsegments,
                labels,
            )
        timings["boundaryRefinementSec"] = time.perf_counter() - boundary_started

        build_started = time.perf_counter()
        segments = _build_diarization_segments(
            speech_segments,
            subsegments,
            labels,
            embeddings,
        )
        timings["buildSegmentsSec"] = time.perf_counter() - build_started
        return SimpleNamespace(
            audio_duration=duration,
            segments=segments,
            speaker_centroids=build_speaker_centroids(embeddings, labels),
            profile=self.build_profile(
                profile,
                timings,
                len(speech_segments),
                len(subsegments),
                len(embeddings),
                {
                    **embedding_stats,
                    "boundaryRefinementWindowCount": len(boundary_subsegments),
                },
            ),
        )

    def build_profile(
        self,
        profile,
        timings,
        speech_segment_count,
        subsegment_count,
        embedding_count,
        embedding_stats,
    ):
        return {
            "embeddingProfile": profile.name,
            "embeddingWindowSec": profile.window_sec,
            "embeddingBaseStepSec": profile.base_step_sec,
            "embeddingLongStepSec": profile.long_step_sec,
            "embeddingLongSegmentThresholdSec": profile.long_segment_threshold_sec,
            "boundaryRefinement": profile.boundary_refinement,
            "speechSegmentCount": speech_segment_count,
            "subsegmentCount": subsegment_count,
            "embeddingCount": embedding_count,
            **embedding_stats,
            "timings": {key: round(value, 6) for key, value in timings.items()},
        }


def load_diarize_function():
    try:
        import diarize  # noqa: F401
    except Exception as exc:
        raise RuntimeError(
            "Python package 'diarize' is not installed. "
            "Install it in the selected environment with: python -m pip install diarize"
        ) from exc
    return ReusableDiarizeEngine()


def build_payload_with_diarize(diarize_fn, audio_path, kwargs):
    started = time.perf_counter()
    result = diarize_fn(str(audio_path), **kwargs)
    wall_clock_sec = time.perf_counter() - started

    return build_payload(
        result,
        backend="diarize",
        model="diarize-0.1.2",
        audio_path=str(audio_path),
        wall_clock_sec=wall_clock_sec,
    )


def run_backend_with_diarize(
    diarize_fn,
    audio_path,
    output_dir,
    num_speakers=None,
    min_speakers=None,
    max_speakers=None,
    embedding_profile=None,
):
    payload = build_payload_with_diarize(
        diarize_fn,
        audio_path,
        backend_kwargs(num_speakers, min_speakers, max_speakers, embedding_profile),
    )

    output_dir.mkdir(parents=True, exist_ok=True)
    diarized_path = output_dir / "diarized-transcription.json"
    report_path = output_dir / "diarize-backend-report.json"
    diarized = {
        "speakers": payload["speakers"],
        "segments": payload["segments"],
    }
    diarized_path.write_text(json.dumps(diarized, ensure_ascii=False, indent=2), encoding="utf-8")
    report = {
        **payload,
        "segmentCount": len(payload["segments"]),
        "speakerCount": len(payload["speakers"]),
        "realtimeFactor": (
            payload["wallClockSec"] / payload["audioDurationSec"]
            if payload["audioDurationSec"] > 0
            else None
        ),
        "speedX": (
            payload["audioDurationSec"] / payload["wallClockSec"]
            if payload["wallClockSec"] > 0
            else None
        ),
        "outputFiles": [str(report_path), str(diarized_path)],
    }
    warnings = dependency_warnings()
    if warnings:
        report["dependencyWarnings"] = warnings
    report_path.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    return report


def run_backend(
    audio_path,
    output_dir,
    num_speakers=None,
    min_speakers=None,
    max_speakers=None,
    embedding_profile=None,
):
    return run_backend_with_diarize(
        load_diarize_function(),
        audio_path,
        output_dir,
        num_speakers=num_speakers,
        min_speakers=min_speakers,
        max_speakers=max_speakers,
        embedding_profile=embedding_profile,
    )


def chunk_value(chunk, *names, default=None):
    for name in names:
        if name in chunk:
            return chunk[name]
    return default


def normalize_max_workers(max_workers, chunk_count):
    if chunk_count <= 0:
        return 1
    if max_workers is None:
        return 1
    try:
        parsed = int(max_workers)
    except (TypeError, ValueError):
        return 1
    return max(1, min(parsed, chunk_count))


def build_batch_chunk_output(diarize_fn, chunk, fallback_index, kwargs):
    audio_path = chunk_value(chunk, "audioPath", "audio_path")
    if not audio_path:
        raise RuntimeError(f"Chunk {fallback_index} is missing audioPath")
    index = int(chunk_value(chunk, "index", default=fallback_index))
    payload = build_payload_with_diarize(diarize_fn, audio_path, kwargs)
    chunk_output = {
        "index": index,
        "audioPath": audio_path,
        "offsetSec": float(chunk_value(chunk, "offsetSec", "offset_sec", default=0.0) or 0.0),
        "diarized": {
            "speakers": payload["speakers"],
            "segments": payload["segments"],
        },
        "report": payload,
    }
    if payload.get("speakerCentroids"):
        chunk_output["speakerCentroids"] = payload["speakerCentroids"]
    return chunk_output


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
    kwargs = backend_kwargs(num_speakers, min_speakers, max_speakers, embedding_profile)
    safe_workers = normalize_max_workers(max_workers, len(chunks))
    started = time.perf_counter()

    if safe_workers <= 1:
        if diarize_fn is None:
            diarize_fn = diarize_factory() if diarize_factory is not None else load_diarize_function()
        chunk_outputs = [
            build_batch_chunk_output(diarize_fn, chunk, fallback_index, kwargs)
            for fallback_index, chunk in enumerate(chunks)
        ]
    else:
        worker_state = threading.local()

        def worker_diarize_fn():
            if diarize_factory is None:
                return diarize_fn
            if not hasattr(worker_state, "diarize_fn"):
                worker_state.diarize_fn = diarize_factory()
            return worker_state.diarize_fn

        def process_chunk(item):
            fallback_index, chunk = item
            local_diarize_fn = worker_diarize_fn()
            if local_diarize_fn is None:
                raise RuntimeError("Parallel batch mode requires a diarize function or factory")
            return build_batch_chunk_output(local_diarize_fn, chunk, fallback_index, kwargs)

        with concurrent.futures.ThreadPoolExecutor(max_workers=safe_workers) as executor:
            futures = [
                executor.submit(process_chunk, item)
                for item in enumerate(chunks)
            ]
            chunk_outputs = [future.result() for future in futures]

    wall_clock_sec = time.perf_counter() - started
    output_dir.mkdir(parents=True, exist_ok=True)
    chunks_path = output_dir / "chunk-diarized-results.json"
    report_path = output_dir / "diarize-batch-backend-report.json"
    chunks_path.write_text(json.dumps(chunk_outputs, ensure_ascii=False, indent=2), encoding="utf-8")
    audio_duration_sec = sum(float(chunk["report"].get("audioDurationSec") or 0.0) for chunk in chunk_outputs)
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
        "realtimeFactor": (
            wall_clock_sec / audio_duration_sec if audio_duration_sec > 0 else None
        ),
        "speedX": (
            audio_duration_sec / wall_clock_sec if wall_clock_sec > 0 else None
        ),
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
                int(profile.get("speechSegmentCount") or 0) for profile in chunk_profiles
            ),
            "boundaryRefinementWindowCount": sum(
                int(profile.get("boundaryRefinementWindowCount") or 0)
                for profile in chunk_profiles
            ),
            "timings": {
                key: round(
                    sum(float(profile.get("timings", {}).get(key) or 0.0) for profile in chunk_profiles),
                    6,
                )
                for key in timing_keys
            },
        }
    warnings = dependency_warnings()
    if warnings:
        report["dependencyWarnings"] = warnings
    report_path.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    return report


def run_backend_batch(
    chunks_path,
    output_dir,
    num_speakers=None,
    min_speakers=None,
    max_speakers=None,
    max_workers=1,
    embedding_profile=None,
):
    chunks = json.loads(Path(chunks_path).read_text(encoding="utf-8"))
    if not isinstance(chunks, list):
        raise RuntimeError("--chunks-json must contain a JSON array")
    safe_workers = normalize_max_workers(max_workers, len(chunks))
    return run_backend_batch_with_diarize(
        None,
        chunks,
        output_dir,
        num_speakers=num_speakers,
        min_speakers=min_speakers,
        max_speakers=max_speakers,
        max_workers=safe_workers,
        diarize_factory=load_diarize_function,
        embedding_profile=embedding_profile,
    )


def parse_args():
    parser = argparse.ArgumentParser(description="Run CPU-only diarize backend benchmark.")
    parser.add_argument("--audio", help="Audio file path")
    parser.add_argument("--chunks-json", help="JSON file with chunk metadata for one-process batch mode")
    parser.add_argument("--out-dir", required=True, help="Output directory")
    parser.add_argument("--num-speakers", type=int)
    parser.add_argument("--min-speakers", type=int)
    parser.add_argument("--max-speakers", type=int)
    parser.add_argument("--max-workers", type=int, default=1, help="Maximum parallel chunks in batch mode")
    parser.add_argument(
        "--embedding-profile",
        choices=["quality", "balanced", "fast", "boundary"],
        help="Embedding density profile. quality preserves legacy dense windows; balanced is the default adaptive mode; boundary enables experimental second-pass boundary refinement.",
    )
    parser.add_argument("--json", action="store_true", help="Print the full JSON report.")
    return parser.parse_args()


def compact_report(report):
    return {
        "backend": report["backend"],
        "model": report["model"],
        "wallClockSec": report["wallClockSec"],
        "maxWorkers": report.get("maxWorkers"),
        "realtimeFactor": report.get("realtimeFactor"),
        "speedX": report.get("speedX"),
        "speakerCount": report.get("speakerCount"),
        "segmentCount": report.get("segmentCount"),
        "outputFiles": report["outputFiles"],
    }


def _test_build_payload():
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
    args = parse_args()
    if args.chunks_json:
        report = run_backend_batch(
            Path(args.chunks_json),
            Path(args.out_dir),
            num_speakers=args.num_speakers,
            min_speakers=args.min_speakers,
            max_speakers=args.max_speakers,
            max_workers=args.max_workers,
            embedding_profile=args.embedding_profile,
        )
    else:
        if not args.audio:
            raise SystemExit("--audio is required unless --chunks-json is used")
        report = run_backend(
            Path(args.audio),
            Path(args.out_dir),
            num_speakers=args.num_speakers,
            min_speakers=args.min_speakers,
            max_speakers=args.max_speakers,
            embedding_profile=args.embedding_profile,
        )
    if args.json:
        print(json.dumps(report, ensure_ascii=False))
    else:
        print(json.dumps(compact_report(report), ensure_ascii=False))
