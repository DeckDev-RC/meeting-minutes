import { useEffect, useMemo, useRef, useState } from "react";
import { listen as tauriListen } from "@tauri-apps/api/event";
import { useParams, useNavigate } from "react-router-dom";
import ProgressPipeline from "../components/ProgressPipeline";
import { useMeetingStore } from "../store/meetingStore";
import {
  alignSpeakerTurnsToTranscription,
  diarizeAudioTurnsModernCpu,
  diarizeAudioTurnsModernCpuChunked,
  diarizeAudioTurnsPyannote,
  diarizeTranscriptionEndToEnd,
  extractFactBatch,
  extractChunkFacts,
  generateAtaFromFactsStreaming,
  getProcessingChunks,
  getApiKeys,
  prepareAudioAndChunks,
  probeMediaMetadata,
  saveTranscription,
  saveMinutes,
  saveProcessingChunks,
  saveBenchmarkRun,
  resolveProcessingWorkDir,
  transcribeChunk,
  updateProcessingChunkFacts,
  updateProcessingChunkResult,
  updateMeetingStatus,
  getMeetings,
  refineDiarizationSelectively,
} from "../lib/tauri";
import { transcribeChunksConcurrently } from "../lib/transcriptionQueue";
import { mergeSortedTranscriptionSegments } from "../lib/segmentMerge";
import { resolveSegmentsForFactScheduling } from "../lib/processingChunks";
import { buildAdaptiveFactBatches, type FactBatchItem } from "../lib/meetingFactsQueue";
import { buildBenchmarkRun, buildBenchmarkRunArtifactPath } from "../lib/benchmarkRun";
import { derivePipelineProgress, type PipelinePhase } from "../lib/pipelineProgress";
import {
  factConcurrencyForPhase,
  transcriptionConcurrencyForProfile,
} from "../lib/processingConcurrency";
import {
  resolveDiarizationExpectedSpeakers,
  shouldPreferChunkedDiarization,
} from "../lib/speakerCount";
import {
  applyLiveTranscriptSpeakers,
  appendLiveInsights,
  appendLiveLog,
  appendLiveTranscript,
  buildLiveMinutesDraft,
  createLiveProcessingState,
  setFinalMinutesText,
  type LiveLogLevel,
  type LiveProcessingState,
  type LiveTab,
} from "../lib/liveProcessing";
import {
  collectExpiredLiveProcessingSnapshotIds,
  collectOverflowLiveProcessingSnapshotIds,
} from "../lib/liveProcessingCache";
import type {
  ExportedChunk,
  MeetingChunkInsights,
  ProcessingProfile,
  ProcessingChunkRecord,
  SpeakerTurn,
  TranscriptionSegment,
} from "../lib/types";

const activeProcessingRuns = new Map<string, Promise<void>>();
const pendingProcessingStartTimers = new Map<string, ReturnType<typeof window.setTimeout>>();
const liveProcessingSnapshots = new Map<string, LiveProcessingState>();
const liveProcessingPublishers = new Map<string, (state: LiveProcessingState) => void>();
const liveProcessingPublishTimers = new Map<string, ReturnType<typeof window.setTimeout>>();
const liveProcessingLastPublishedAt = new Map<string, number>();
const liveProcessingTouchedAt = new Map<string, number>();
const completedProcessingSnapshots = new Map<string, number>();
const minutesStreamRawSnapshots = new Map<string, string>();
const liveProcessingParticipantNames = new Map<string, string[]>();
let visibleProcessingMeetingId: string | null = null;

const LIVE_STATE_PUBLISH_INTERVAL_MS = 160;
const COMPLETED_SNAPSHOT_TTL_MS = 10 * 60 * 1000;
const COMPLETED_SNAPSHOT_CLEANUP_INTERVAL_MS = 5 * 60 * 1000;
const MAX_LIVE_PROCESSING_SNAPSHOTS = 5;

type MinutesStreamPayload = {
  meetingId: string;
  delta: string;
  done: boolean;
};

type FactQueueItem = {
  chunk: ProcessingChunkRecord;
  segments: TranscriptionSegment[];
  segmentsJson: string;
};

const joinPath = (dir: string, fileName: string) => {
  const separator = dir.includes("\\") ? "\\" : "/";
  return `${dir.replace(/[\\/]+$/, "")}${separator}${fileName}`;
};

const flushLiveStateSnapshot = (meetingId: string) => {
  const timer = liveProcessingPublishTimers.get(meetingId);
  if (timer) {
    window.clearTimeout(timer);
    liveProcessingPublishTimers.delete(meetingId);
  }
  const publish = liveProcessingPublishers.get(meetingId);
  const snapshot = liveProcessingSnapshots.get(meetingId);
  if (visibleProcessingMeetingId === meetingId && publish && snapshot) {
    const participantNames = liveProcessingParticipantNames.get(meetingId) ?? [];
    const publishSnapshot =
      snapshot.finalMinutesText || snapshot.insights.length === 0
        ? snapshot
        : {
            ...snapshot,
            minutesDraft: buildLiveMinutesDraft(snapshot.insights, participantNames),
          };
    if (publishSnapshot !== snapshot) {
      liveProcessingSnapshots.set(meetingId, publishSnapshot);
    }
    liveProcessingLastPublishedAt.set(meetingId, Date.now());
    publish(publishSnapshot);
  }
};

const scheduleLiveStatePublish = (meetingId: string) => {
  if (visibleProcessingMeetingId !== meetingId || !liveProcessingPublishers.has(meetingId)) {
    return;
  }
  const now = Date.now();
  const lastPublishedAt = liveProcessingLastPublishedAt.get(meetingId) ?? 0;
  const waitMs = LIVE_STATE_PUBLISH_INTERVAL_MS - (now - lastPublishedAt);

  if (waitMs <= 0) {
    flushLiveStateSnapshot(meetingId);
    return;
  }

  if (!liveProcessingPublishTimers.has(meetingId)) {
    const timer = window.setTimeout(() => {
      liveProcessingPublishTimers.delete(meetingId);
      flushLiveStateSnapshot(meetingId);
    }, waitMs);
    liveProcessingPublishTimers.set(meetingId, timer);
  }
};

const cleanupCompletedLiveProcessingSnapshots = () => {
  const activeMeetingIds = new Set(activeProcessingRuns.keys());
  const expiredIds = collectExpiredLiveProcessingSnapshotIds({
    completedAtByMeetingId: completedProcessingSnapshots,
    activeMeetingIds,
    visibleMeetingId: visibleProcessingMeetingId,
    nowMs: Date.now(),
    ttlMs: COMPLETED_SNAPSHOT_TTL_MS,
  });
  const overflowIds = collectOverflowLiveProcessingSnapshotIds({
    snapshotIds: Array.from(liveProcessingSnapshots.keys()),
    activeMeetingIds,
    visibleMeetingId: visibleProcessingMeetingId,
    touchedAtByMeetingId: liveProcessingTouchedAt,
    maxEntries: MAX_LIVE_PROCESSING_SNAPSHOTS,
  });

  for (const meetingId of new Set([...expiredIds, ...overflowIds])) {
    const timer = liveProcessingPublishTimers.get(meetingId);
    if (timer) {
      window.clearTimeout(timer);
      liveProcessingPublishTimers.delete(meetingId);
    }
    liveProcessingSnapshots.delete(meetingId);
    minutesStreamRawSnapshots.delete(meetingId);
    liveProcessingParticipantNames.delete(meetingId);
    completedProcessingSnapshots.delete(meetingId);
    liveProcessingLastPublishedAt.delete(meetingId);
    liveProcessingTouchedAt.delete(meetingId);
  }
};

const markProcessingSnapshotCompleted = (meetingId: string) => {
  completedProcessingSnapshots.set(meetingId, Date.now());
  flushLiveStateSnapshot(meetingId);
  cleanupCompletedLiveProcessingSnapshots();
};

const chunkOverlapForProfile = (profile: ProcessingProfile) => {
  if (profile === "turbo") return 0;
  return 3;
};

const formatError = (err: unknown) => {
  if (typeof err === "string") return err.trim();
  if (err instanceof Error) return err.message.trim();
  if (err && typeof err === "object") {
    const maybeMessage = "message" in err ? String((err as { message?: unknown }).message ?? "") : "";
    if (maybeMessage.trim()) return maybeMessage.trim();
    try {
      return JSON.stringify(err);
    } catch {
      return String(err);
    }
  }
  return "";
};

const toExportedChunk = (chunk: ProcessingChunkRecord): ExportedChunk => ({
  index: chunk.index,
  audioPath: chunk.audioPath,
  startSec: chunk.startSec,
  endSec: chunk.endSec,
  offsetSec: chunk.offsetSec,
  durationSec: chunk.durationSec,
});

const toNewProcessingChunkRecord = (
  meetingId: string,
  chunk: ExportedChunk,
): ProcessingChunkRecord => ({
  meetingId,
  index: chunk.index,
  audioPath: chunk.audioPath,
  startSec: chunk.startSec,
  endSec: chunk.endSec,
  offsetSec: chunk.offsetSec,
  durationSec: chunk.durationSec,
  status: "pending",
  rawSegmentsJson: null,
  errorMsg: null,
  factsStatus: "pending",
  factsJson: null,
  factsErrorMsg: null,
});

const sumChunkDurations = (chunks: Pick<ProcessingChunkRecord, "durationSec">[]) =>
  chunks.reduce((sum, chunk) => sum + chunk.durationSec, 0);

const isTranscriptionSegment = (value: unknown): value is TranscriptionSegment => {
  if (!value || typeof value !== "object") return false;
  const segment = value as Partial<TranscriptionSegment>;
  return (
    typeof segment.id === "number" &&
    typeof segment.start === "number" &&
    typeof segment.end === "number" &&
    typeof segment.text === "string"
  );
};

const parseStoredSegments = (chunk: ProcessingChunkRecord): TranscriptionSegment[] => {
  if (!chunk.rawSegmentsJson) {
    throw new Error(`Trecho ${chunk.index} marcado como concluido sem transcricao salva.`);
  }

  try {
    const parsed = JSON.parse(chunk.rawSegmentsJson) as unknown;
    if (!Array.isArray(parsed) || !parsed.every(isTranscriptionSegment)) {
      throw new Error("formato inesperado");
    }
    return parsed;
  } catch (err) {
    throw new Error(
      `Transcricao salva do trecho ${chunk.index} esta invalida: ${
        formatError(err) || "JSON invalido"
      }`,
    );
  }
};

const isMeetingChunkInsights = (value: unknown): value is MeetingChunkInsights => {
  if (!value || typeof value !== "object") return false;
  const item = value as Partial<MeetingChunkInsights>;
  return (
    typeof item.chunkIndex === "number" &&
    typeof item.startSec === "number" &&
    typeof item.endSec === "number" &&
    typeof item.summary === "string" &&
    Array.isArray(item.topics) &&
    Array.isArray(item.decisions) &&
    Array.isArray(item.actions) &&
    Array.isArray(item.questions) &&
    Array.isArray(item.risks)
  );
};

const parseCachedFacts = (chunk: ProcessingChunkRecord): MeetingChunkInsights | null => {
  if (chunk.factsStatus !== "done" || !chunk.factsJson) return null;
  try {
    const parsed = JSON.parse(chunk.factsJson) as unknown;
    return isMeetingChunkInsights(parsed) ? parsed : null;
  } catch {
    return null;
  }
};

type SpeculativeSpeakerTurns = {
  turns: SpeakerTurn[];
  error: string;
  engine: "pyannote" | "modern-cpu" | "modern-cpu-chunked" | "none";
  fallbackReason?: string;
};

const summarizeBackendError = (message: string) => {
  const clean = message.replace(/\r/g, "").trim();
  if (!clean) return "";
  if (clean.includes("Please enable access to public gated repositories")) {
    return "Token Hugging Face sem permissao para repositorios publicos gated.";
  }
  if (clean.includes("403 Forbidden")) {
    return "Hugging Face retornou 403 para o modelo pyannote.";
  }
  if (clean.includes("HF_TOKEN is not available")) {
    return "HF_TOKEN nao esta disponivel para o worker pyannote.";
  }
  if (clean.includes("pyannote.audio") && clean.includes("not installed")) {
    return "Ambiente pyannote nao esta instalado.";
  }
  return clean.split("\n").find((line) => line.trim())?.trim().slice(0, 180) || "";
};

const startSpeculativeSpeakerTurns = (
  audioPath: string,
  expectedSpeakers?: number,
  audioChunks: ExportedChunk[] = [],
  preferChunked = false,
  preferPyannote = false,
): Promise<SpeculativeSpeakerTurns> => {
  if (preferChunked && expectedSpeakers && audioChunks.length > 1) {
    return diarizeAudioTurnsModernCpuChunked(audioChunks, expectedSpeakers, 2)
      .then((turns) => ({ turns, error: "", engine: "modern-cpu-chunked" as const }))
      .catch((chunkedErr) =>
        diarizeAudioTurnsModernCpu(audioPath, expectedSpeakers)
          .then((turns) => ({
            turns,
            error: "",
            engine: "modern-cpu" as const,
            fallbackReason: summarizeBackendError(formatError(chunkedErr)),
          }))
          .catch((err) => ({
            turns: [],
            engine: "none" as const,
            error: formatError(err) || "Diarizacao local indisponivel.",
            fallbackReason: summarizeBackendError(formatError(chunkedErr)),
          })),
      );
  }

  if (preferPyannote) {
    return diarizeAudioTurnsPyannote(audioPath, expectedSpeakers)
      .then((turns) => ({ turns, error: "", engine: "pyannote" as const }))
      .catch((pyannoteErr) =>
        diarizeAudioTurnsModernCpu(audioPath, expectedSpeakers)
          .then((turns) => ({
            turns,
            error: "",
            engine: "modern-cpu" as const,
            fallbackReason: summarizeBackendError(formatError(pyannoteErr)),
          }))
          .catch((err) => ({
            turns: [],
            engine: "none" as const,
            error: formatError(err) || "Diarizacao local indisponivel.",
            fallbackReason: summarizeBackendError(formatError(pyannoteErr)),
          })),
      );
  }

  return diarizeAudioTurnsModernCpu(audioPath, expectedSpeakers)
    .then((turns) => ({ turns, error: "", engine: "modern-cpu" as const }))
    .catch((err) => ({
      turns: [],
      engine: "none" as const,
      error: formatError(err) || "Diarizacao local indisponivel.",
    }));
};

const PROFILE_LABELS: Record<ProcessingProfile, string> = {
  turbo: "Turbo",
  balanced: "Equilibrado",
  precision: "Precisao",
};

const normalizeProcessingProfile = (value: string | null | undefined): ProcessingProfile => {
  if (value === "turbo" || value === "precision") return value;
  return "balanced";
};

const parseParticipantsHint = (hint: string | null | undefined) =>
  (hint || "")
    .split(/[\n,;]+/)
    .map((name) => name.trim())
    .filter(Boolean)
    .slice(0, 30);

const htmlToReadablePreview = (html: string) =>
  html
    .replace(/<style[\s\S]*?<\/style>/gi, " ")
    .replace(/<script[\s\S]*?<\/script>/gi, " ")
    .replace(/<\/(p|div|section|h[1-6]|li|tr)>/gi, "\n")
    .replace(/<[^>]+>/g, " ")
    .replace(/&nbsp;/g, " ")
    .replace(/&amp;/g, "&")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/[ \t]+/g, " ")
    .replace(/\n{3,}/g, "\n\n")
    .trim();

const liveLogBadgeClass = (level: LiveLogLevel) => {
  if (level === "success") return "bg-emerald-50 text-emerald-700 ring-emerald-200";
  if (level === "warning") return "bg-amber-50 text-amber-700 ring-amber-200";
  if (level === "error") return "bg-red-50 text-red-700 ring-red-200";
  return "bg-blue-50 text-blue-700 ring-blue-200";
};

const listenToAppEvent = <T,>(eventName: string, handler: (event: { payload: T }) => void) => {
  const mock = window.__MEETING_MINUTES_E2E__?.listen;
  if (mock) {
    return mock<T>(eventName, handler);
  }

  return tauriListen<T>(eventName, handler);
};

export default function Processing() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const startedAtRef = useRef<number>(Date.now());
  const lastProgressRenderRef = useRef<number>(0);
  const livePanelScrollRef = useRef<HTMLDivElement | null>(null);
  const transcriptAutoScrollRef = useRef(true);
  const pendingProgressRef = useRef<{
    percent: number;
    title: string;
    detail: string;
    etaLabel: string;
    speedLabel: string;
  } | null>(null);
  const progressTimerRef = useRef<ReturnType<typeof window.setTimeout> | null>(null);
  const minutesStreamRenderTimerRef = useRef<ReturnType<typeof window.setTimeout> | null>(null);
  const minutesStreamRawRef = useRef("");
  const [runProfile, setRunProfile] = useState<ProcessingProfile>("balanced");
  const [liveState, setLiveState] = useState<LiveProcessingState>(() =>
    createLiveProcessingState(),
  );
  const [liveTab, setLiveTab] = useState<LiveTab>("transcript");
  const {
    stepStatus,
    currentStep,
    progress,
    progressTitle,
    progressDetail,
    progressEta,
    progressSpeed,
    processingNote,
    error,
    setCurrentMeeting,
    setStep,
    setStepStatus,
    setProgressStats,
    setProcessingNote,
    setError,
    setMeetings,
  } = useMeetingStore();

  const commitLiveState = (
    meetingId: string,
    updater: (state: LiveProcessingState) => LiveProcessingState,
  ) => {
    const current = liveProcessingSnapshots.get(meetingId) ?? createLiveProcessingState();
    const next = updater(current);
    liveProcessingSnapshots.set(meetingId, next);
    liveProcessingTouchedAt.set(meetingId, Date.now());
    completedProcessingSnapshots.delete(meetingId);
    scheduleLiveStatePublish(meetingId);
  };

  const addLiveLog = (meetingId: string, level: LiveLogLevel, message: string) => {
    commitLiveState(meetingId, (state) =>
      appendLiveLog(state, level, message, (Date.now() - startedAtRef.current) / 1000),
    );
  };

  const flushMinutesStreamPreview = (meetingId: string) => {
    if (minutesStreamRenderTimerRef.current) {
      window.clearTimeout(minutesStreamRenderTimerRef.current);
      minutesStreamRenderTimerRef.current = null;
    }
    commitLiveState(meetingId, (state) =>
      setFinalMinutesText(state, htmlToReadablePreview(minutesStreamRawRef.current)),
    );
  };

  const scheduleMinutesStreamPreview = (meetingId: string) => {
    if (minutesStreamRenderTimerRef.current) return;
    minutesStreamRenderTimerRef.current = window.setTimeout(() => {
      minutesStreamRenderTimerRef.current = null;
      flushMinutesStreamPreview(meetingId);
    }, 180);
  };

  const flushPendingProgress = () => {
    const pending = pendingProgressRef.current;
    if (!pending) {
      return;
    }

    pendingProgressRef.current = null;
    lastProgressRenderRef.current = Date.now();
    setProgressStats(
      pending.percent,
      pending.title,
      pending.detail,
      pending.etaLabel,
      pending.speedLabel,
    );
  };

  const updatePipelineProgress = (
    phase: PipelinePhase,
    completedAudioSec: number,
    totalAudioSec: number,
    completedChunks: number,
    totalChunks: number,
  ) => {
    const view = derivePipelineProgress({
      phase,
      completedAudioSec,
      totalAudioSec,
      completedChunks,
      totalChunks,
      elapsedMs: Date.now() - startedAtRef.current,
    });
    const now = Date.now();
    const shouldRenderNow =
      phase === "complete" ||
      totalChunks === 0 ||
      completedChunks >= totalChunks ||
      now - lastProgressRenderRef.current >= 500;

    pendingProgressRef.current = view;
    if (shouldRenderNow) {
      if (progressTimerRef.current) {
        window.clearTimeout(progressTimerRef.current);
        progressTimerRef.current = null;
      }
      flushPendingProgress();
      return;
    }

    if (!progressTimerRef.current) {
      progressTimerRef.current = window.setTimeout(() => {
        progressTimerRef.current = null;
        flushPendingProgress();
      }, 500 - (now - lastProgressRenderRef.current));
    }
  };

  const transcriptCount = liveState.transcript.length;
  const insightCount = liveState.insights.length;
  const logCount = liveState.logs.length;
  const hasMinutesPreview = Boolean(liveState.finalMinutesText || liveState.minutesDraft);
  const liveTabItems = useMemo(
    () => [
      { key: "transcript" as const, label: "Transcricao", count: transcriptCount },
      { key: "insights" as const, label: "Insights", count: insightCount },
      {
        key: "minutes" as const,
        label: "Ata",
        count: hasMinutesPreview ? 1 : 0,
      },
      { key: "logs" as const, label: "Logs tecnicos", count: logCount },
    ],
    [hasMinutesPreview, insightCount, logCount, transcriptCount],
  );

  useEffect(() => {
    if (liveTab !== "transcript") return;
    const container = livePanelScrollRef.current;
    if (!container) return;
    container.scrollTo({
      top: transcriptAutoScrollRef.current ? container.scrollHeight : 0,
      behavior: transcriptAutoScrollRef.current ? "smooth" : "auto",
    });
  }, [liveTab, transcriptCount]);

  useEffect(() => {
    const cleanupTimer = window.setInterval(
      cleanupCompletedLiveProcessingSnapshots,
      COMPLETED_SNAPSHOT_CLEANUP_INTERVAL_MS,
    );
    return () => window.clearInterval(cleanupTimer);
  }, []);

  useEffect(() => {
    if (!id) return;
    visibleProcessingMeetingId = id;
    setCurrentMeeting(id);
    const snapshot = liveProcessingSnapshots.get(id) ?? createLiveProcessingState();
    liveProcessingSnapshots.set(id, snapshot);
    liveProcessingTouchedAt.set(id, Date.now());
    liveProcessingPublishers.set(id, setLiveState);
    setLiveState(snapshot);
    liveProcessingLastPublishedAt.set(id, Date.now());
    transcriptAutoScrollRef.current = true;
    setLiveTab("transcript");
    minutesStreamRawRef.current = minutesStreamRawSnapshots.get(id) ?? "";

    let disposed = false;
    let unlistenStream: (() => void) | null = null;
    void listenToAppEvent<MinutesStreamPayload>("meeting-minutes://minutes-stream", (event) => {
      const payload = event.payload;
      if (payload.meetingId !== id) return;
      if (payload.delta) {
        const raw = `${minutesStreamRawSnapshots.get(id) ?? ""}${payload.delta}`;
        minutesStreamRawSnapshots.set(id, raw);
        minutesStreamRawRef.current = raw;
        scheduleMinutesStreamPreview(id);
      }
      if (payload.done) {
        flushMinutesStreamPreview(id);
        addLiveLog(id, "success", "Ata final recebida.");
      }
    }).then((unlisten) => {
      if (disposed) {
        unlisten();
      } else {
        unlistenStream = unlisten;
      }
    });

    if (!activeProcessingRuns.has(id) && !pendingProcessingStartTimers.has(id)) {
      const timer = window.setTimeout(() => {
        pendingProcessingStartTimers.delete(id);
        if (activeProcessingRuns.has(id)) return;

        const run = runPipeline(id).finally(() => {
          activeProcessingRuns.delete(id);
          cleanupCompletedLiveProcessingSnapshots();
        });
        activeProcessingRuns.set(id, run);
      }, 50);
      pendingProcessingStartTimers.set(id, timer);
    }

    return () => {
      const pendingStart = pendingProcessingStartTimers.get(id);
      if (pendingStart) {
        window.clearTimeout(pendingStart);
        pendingProcessingStartTimers.delete(id);
      }
      if (progressTimerRef.current) {
        window.clearTimeout(progressTimerRef.current);
        progressTimerRef.current = null;
      }
      if (minutesStreamRenderTimerRef.current) {
        window.clearTimeout(minutesStreamRenderTimerRef.current);
        minutesStreamRenderTimerRef.current = null;
      }
      const livePublishTimer = liveProcessingPublishTimers.get(id);
      if (livePublishTimer) {
        window.clearTimeout(livePublishTimer);
        liveProcessingPublishTimers.delete(id);
      }
      disposed = true;
      unlistenStream?.();
      if (liveProcessingPublishers.get(id) === setLiveState) {
        liveProcessingPublishers.delete(id);
      }
      if (visibleProcessingMeetingId === id) {
        visibleProcessingMeetingId = null;
      }
    };
  }, [id]);

  const runPipeline = async (meetingId: string) => {
    try {
      startedAtRef.current = Date.now();
      lastProgressRenderRef.current = 0;
      pendingProgressRef.current = null;
      if (progressTimerRef.current) {
        window.clearTimeout(progressTimerRef.current);
        progressTimerRef.current = null;
      }
      if (minutesStreamRenderTimerRef.current) {
        window.clearTimeout(minutesStreamRenderTimerRef.current);
        minutesStreamRenderTimerRef.current = null;
      }
      const freshLiveState = createLiveProcessingState();
      liveProcessingSnapshots.set(meetingId, freshLiveState);
      completedProcessingSnapshots.delete(meetingId);
      minutesStreamRawSnapshots.set(meetingId, "");
      minutesStreamRawRef.current = "";
      setLiveState(freshLiveState);
      liveProcessingLastPublishedAt.set(meetingId, Date.now());
      transcriptAutoScrollRef.current = true;
      setLiveTab("transcript");
      setCurrentMeeting(meetingId);
      setError(null);
      setProcessingNote("");
      addLiveLog(meetingId, "info", "Processamento iniciado.");
      setStepStatus("extract_audio", "pending");
      setStepStatus("transcribe", "pending");
      setStepStatus("diarize", "pending");
      setStepStatus("generate", "pending");
      setStepStatus("pdf", "pending");
      updatePipelineProgress("prepare_audio", 0, 0, 0, 0);

      const keys = await getApiKeys();
      if (!keys.groq || !keys.gemini) {
        setError("Configure suas chaves de API em Configuracoes antes de processar.");
        return;
      }

      const meetings = await getMeetings();
      const meeting = meetings.find((m) => m.id === meetingId);
      if (!meeting) {
        setError("Reuniao nao encontrada.");
        return;
      }
      const processingProfile = normalizeProcessingProfile(meeting.processingProfile);
      const participantNames = parseParticipantsHint(meeting.participantsHint);
      const diarizationExpectedSpeakers = resolveDiarizationExpectedSpeakers(
        keys.expectedSpeakers,
        participantNames,
      );
      const inferredExpectedSpeakers =
        !keys.expectedSpeakers && diarizationExpectedSpeakers !== undefined;
      liveProcessingParticipantNames.set(meetingId, participantNames);
      const meetingMetadata = await probeMediaMetadata(meeting.filePath).catch((err) => {
        console.warn("Failed to probe media metadata:", err);
        return {
          sourcePath: meeting.filePath,
          sourceFileName: meeting.filePath.split(/[\\/]/).pop() || null,
        };
      });
      const speakerProcessingNote = diarizationExpectedSpeakers
        ? `Motor CPU moderno com ${diarizationExpectedSpeakers} falantes esperados; usa blocos quando possivel.`
        : processingProfile === "precision"
          ? "Precisao: CPU moderno em blocos quando ha numero esperado de falantes; depois refina trechos suspeitos."
          : processingProfile === "turbo"
            ? "Turbo: chunks sem overlap para exportacao em lote, transcricao mais concorrente e sem refinamento seletivo."
            : "Motor CPU moderno ativo para identificar falantes em paralelo.";
      setRunProfile(processingProfile);
      setProcessingNote(speakerProcessingNote);
      addLiveLog(meetingId, "info", `Perfil ${PROFILE_LABELS[processingProfile]} selecionado.`);
      if (inferredExpectedSpeakers) {
        addLiveLog(
          meetingId,
          "info",
          `Numero esperado de falantes inferido pelos participantes informados: ${diarizationExpectedSpeakers}.`,
        );
      }

      if (meeting.status === "done") {
        setStepStatus("extract_audio", "done");
        setStepStatus("transcribe", "done");
        setStepStatus("diarize", "done");
        setStepStatus("generate", "done");
        updatePipelineProgress("complete", 1, 1, 1, 1);
        if (visibleProcessingMeetingId === meetingId) {
          navigate(`/minutes/${meetingId}`);
        }
        return;
      }

      await updateMeetingStatus(meetingId, "processing");

      // Step 1: Extract audio and create/resume smart chunks.
      setStep("extract_audio");
      setStepStatus("extract_audio", "running");
      const processingWorkDir = await resolveProcessingWorkDir(meetingId);
      const audioOutput = joinPath(processingWorkDir, `${meetingId}_audio.wav`);
      let storedChunks = await getProcessingChunks(meetingId);
      let durationSec = sumChunkDurations(storedChunks);

      if (storedChunks.length === 0) {
        addLiveLog(meetingId, "info", "Extraindo audio e detectando pausas.");
        updatePipelineProgress("detect_speech", 0, 0, 0, 0);
        const chunkDir = joinPath(processingWorkDir, "chunks");
        addLiveLog(
          meetingId,
          "info",
          "Preparando audio normalizado e chunks inteligentes em workspace local.",
        );
        const prepared = await prepareAudioAndChunks(meeting.filePath, audioOutput, chunkDir, {
          targetSec: 360,
          minSec: 180,
          maxSec: 480,
          overlapSec: chunkOverlapForProfile(processingProfile),
          silenceMinDurationSec: 0.45,
          silenceNoiseDb: -35,
          outputFormat: "flac",
        });
        durationSec = prepared.durationSec;
        updatePipelineProgress("detect_speech", 0, durationSec, 0, 0);
        updatePipelineProgress("create_chunks", 0, durationSec, 0, 0);
        await saveProcessingChunks(meetingId, prepared.chunks);
        storedChunks = prepared.chunks.map((chunk) =>
          toNewProcessingChunkRecord(meetingId, chunk),
        );
        durationSec = sumChunkDurations(storedChunks);
        addLiveLog(meetingId, "success", `${storedChunks.length} chunks criados.`);
      }

      if (storedChunks.length === 0) {
        throw new Error("Nenhum trecho de audio foi criado para transcricao.");
      }

      const storedChunkByIndex = new Map(storedChunks.map((chunk) => [chunk.index, chunk]));
      const exportedChunks = storedChunks.map(toExportedChunk);
      const patchStoredChunk = (chunkIndex: number, patch: Partial<ProcessingChunkRecord>) => {
        const record = storedChunkByIndex.get(chunkIndex);
        if (record) {
          Object.assign(record, patch);
        }
        return record;
      };
      const totalAudioSec = durationSec;
      const completedStoredChunks = storedChunks.filter((chunk) => chunk.status === "done");
      const completedSegmentsByChunk = completedStoredChunks.map((chunk) => ({
        chunk,
        segments: parseStoredSegments(chunk),
      }));
      const parsedSegmentsByChunk = new Map(
        completedSegmentsByChunk.map(({ chunk, segments }) => [chunk.index, segments]),
      );
      const segmentJsonByChunk = new Map(
        completedStoredChunks.flatMap((chunk) =>
          chunk.rawSegmentsJson ? [[chunk.index, chunk.rawSegmentsJson] as const] : [],
        ),
      );
      const completedSegments = completedSegmentsByChunk.flatMap((item) => item.segments);
      for (const { chunk, segments } of completedSegmentsByChunk) {
        commitLiveState(meetingId, (state) =>
          appendLiveTranscript(state, chunk.index, segments),
        );
      }
      const completedAudioSec = sumChunkDurations(completedStoredChunks);
      let transcribedAudioSec = completedAudioSec;
      let transcribedChunkCount = completedStoredChunks.length;
      const pendingChunks = storedChunks.flatMap((chunk, index) =>
        chunk.status !== "done" ? [exportedChunks[index]] : [],
      );
      const pendingChunkByPath = new Map(pendingChunks.map((chunk) => [chunk.audioPath, chunk]));
      const factResults = new Map<number, MeetingChunkInsights>();
      const factQueue: FactQueueItem[] = [];
      const factBatchQueue: FactBatchItem[][] = [];
      let factConcurrencyLimit = factConcurrencyForPhase(processingProfile, false);
      let activeFactWorkers = 0;
      let completedFactChunks = 0;
      let factsInputClosed = false;
      let factsFailed = false;
      let factsPhaseVisible = false;
      let resolveFacts!: (facts: MeetingChunkInsights[]) => void;
      let rejectFacts!: (error: unknown) => void;
      const factsReadyPromise = new Promise<MeetingChunkInsights[]>((resolve, reject) => {
        resolveFacts = resolve;
        rejectFacts = reject;
      });
      factsReadyPromise.catch(() => {});

      const orderedFactResults = () =>
        storedChunks.flatMap((chunk) => factResults.get(chunk.index) ?? []);

      const reportFactProgress = () => {
        if (!factsPhaseVisible) return;
        updatePipelineProgress(
          "extract_facts",
          totalAudioSec,
          totalAudioSec,
          completedFactChunks,
          storedChunks.length,
        );
      };

      const maybeResolveFacts = () => {
        if (
          !factsFailed &&
          factsInputClosed &&
          activeFactWorkers === 0 &&
          factQueue.length === 0 &&
          factBatchQueue.length === 0
        ) {
          resolveFacts(orderedFactResults());
        }
      };

      const drainFactQueue = () => {
        while (
          !factsFailed &&
          activeFactWorkers < factConcurrencyLimit &&
          (factBatchQueue.length > 0 || factQueue.length > 0)
        ) {
          const batch = factBatchQueue.shift();
          if (batch) {
            activeFactWorkers += 1;
            void (async () => {
              const runningPersists = batch.map((item) =>
                updateProcessingChunkFacts(meetingId, item.chunk.index, "running").catch(() => {}),
              );
              try {
                for (const item of batch) {
                  patchStoredChunk(item.chunk.index, { factsStatus: "running" });
                }
                const insightsList = await extractFactBatch(batch, keys.gemini, participantNames);
                const insightsByChunk = new Map(
                  insightsList.map((insights) => [insights.chunkIndex, insights]),
                );
                await Promise.all(runningPersists);

                for (const item of batch) {
                  const insights = insightsByChunk.get(item.chunk.index) ?? {
                    chunkIndex: item.chunk.index,
                    startSec: item.chunk.startSec,
                    endSec: item.chunk.endSec,
                    summary: item.segments.map((segment) => segment.text).join(" ").slice(0, 420),
                    topics: [],
                    decisions: [],
                    actions: [],
                    questions: [],
                    risks: [],
                  };
                  commitLiveState(meetingId, (state) =>
                    appendLiveInsights(state, insights, undefined, participantNames),
                  );
                  const factsJson = JSON.stringify(insights);
                  await updateProcessingChunkFacts(meetingId, item.chunk.index, "done", factsJson);
                  factResults.set(item.chunk.index, insights);
                  patchStoredChunk(item.chunk.index, {
                    factsStatus: "done",
                    factsJson,
                    factsErrorMsg: null,
                  });
                  completedFactChunks += 1;
                }
                addLiveLog(meetingId, "success", `${batch.length} chunks de insights extraidos em lote.`);
                reportFactProgress();
              } catch (err) {
                factsFailed = true;
                addLiveLog(meetingId, "error", `Falha nos insights em lote: ${formatError(err)}`);
                await Promise.all(runningPersists);
                await Promise.all(
                  batch.map((item) =>
                    updateProcessingChunkFacts(
                      meetingId,
                      item.chunk.index,
                      "error",
                      undefined,
                      formatError(err),
                    ).catch(() => {}),
                  ),
                );
                for (const item of batch) {
                  patchStoredChunk(item.chunk.index, {
                    factsStatus: "error",
                    factsErrorMsg: formatError(err),
                  });
                }
                rejectFacts(err);
              } finally {
                activeFactWorkers -= 1;
                drainFactQueue();
                maybeResolveFacts();
              }
            })();
            continue;
          }

          const { chunk, segmentsJson } = factQueue.shift()!;
          activeFactWorkers += 1;
          void (async () => {
            let runningPersist = Promise.resolve();
            try {
              runningPersist = updateProcessingChunkFacts(
                meetingId,
                chunk.index,
                "running",
              ).catch(() => {});
              patchStoredChunk(chunk.index, { factsStatus: "running" });
              const insights = await extractChunkFacts(
                chunk.index,
                chunk.startSec,
                chunk.endSec,
                segmentsJson,
                keys.gemini,
                participantNames,
              );
              commitLiveState(meetingId, (state) =>
                appendLiveInsights(state, insights, undefined, participantNames),
              );
              addLiveLog(meetingId, "success", `Insights do chunk ${chunk.index + 1} extraidos.`);
              const factsJson = JSON.stringify(insights);
              await runningPersist;
              await updateProcessingChunkFacts(meetingId, chunk.index, "done", factsJson);
              factResults.set(chunk.index, insights);
              patchStoredChunk(chunk.index, {
                factsStatus: "done",
                factsJson,
                factsErrorMsg: null,
              });
              completedFactChunks += 1;
              reportFactProgress();
            } catch (err) {
              factsFailed = true;
              addLiveLog(
                meetingId,
                "error",
                `Falha nos insights do chunk ${chunk.index + 1}: ${formatError(err)}`,
              );
              await runningPersist;
              await updateProcessingChunkFacts(
                meetingId,
                chunk.index,
                "error",
                undefined,
                formatError(err),
              ).catch(() => {});
              patchStoredChunk(chunk.index, {
                factsStatus: "error",
                factsErrorMsg: formatError(err),
              });
              rejectFacts(err);
            } finally {
              activeFactWorkers -= 1;
              drainFactQueue();
              maybeResolveFacts();
            }
          })();
        }
      };

      const scheduleFactExtraction = (
        chunk: ProcessingChunkRecord,
        knownSegments = parsedSegmentsByChunk.get(chunk.index),
      ) => {
        if (factsFailed || factResults.has(chunk.index)) return;
        const segmentsForChunk = resolveSegmentsForFactScheduling(
          chunk,
          knownSegments,
          parseStoredSegments,
        );
        if (!segmentsForChunk) return;
        const cached = parseCachedFacts(chunk);
        if (cached) {
          factResults.set(chunk.index, cached);
          completedFactChunks += 1;
          commitLiveState(meetingId, (state) =>
            appendLiveInsights(state, cached, undefined, participantNames),
          );
          reportFactProgress();
          return;
        }
        parsedSegmentsByChunk.set(chunk.index, segmentsForChunk);
        const segmentsJson =
          segmentJsonByChunk.get(chunk.index) ?? JSON.stringify(segmentsForChunk);
        segmentJsonByChunk.set(chunk.index, segmentsJson);
        factQueue.push({ chunk, segments: segmentsForChunk, segmentsJson });
        drainFactQueue();
      };

      const closeFactInput = () => {
        factsInputClosed = true;
        factConcurrencyLimit = factConcurrencyForPhase(processingProfile, true);
        if (factQueue.length > 1) {
          const pendingItems = factQueue.splice(0, factQueue.length);
          const byChunkIndex = new Map(pendingItems.map((item) => [item.chunk.index, item]));
          const batches = buildAdaptiveFactBatches({
            chunks: pendingItems.map((item) => item.chunk),
            parseSegments: (chunk) => byChunkIndex.get(chunk.index)?.segments ?? [],
          });
          factBatchQueue.push(...batches.map((batch) => batch.items));
        }
        drainFactQueue();
        maybeResolveFacts();
        return factsReadyPromise;
      };

      for (const chunk of storedChunks) {
        scheduleFactExtraction(chunk);
      }
      setStepStatus("extract_audio", "done");
      setStepStatus("diarize", "running");
      addLiveLog(meetingId, "info", "Identificacao de falantes iniciada em paralelo.");
      const preferChunkedSpeakerTurns = shouldPreferChunkedDiarization(
        diarizationExpectedSpeakers,
        exportedChunks.length,
      );
      if (preferChunkedSpeakerTurns) {
        addLiveLog(meetingId, "info", "Diarizacao CPU em blocos ativada para esta reuniao.");
        setProcessingNote(
          `CPU moderno em blocos ativo com ${diarizationExpectedSpeakers} falantes esperados.`,
        );
      }
      const speakerTurnsStartedAt = Date.now();
      const speakerTurnsPromise = startSpeculativeSpeakerTurns(
        audioOutput,
        diarizationExpectedSpeakers,
        exportedChunks,
        preferChunkedSpeakerTurns,
        false,
      ).then((result) => {
        const elapsedSec = (Date.now() - speakerTurnsStartedAt) / 1000;
        addLiveLog(
          meetingId,
          result.error ? "warning" : "success",
          `Motor de falantes ${result.engine} concluiu em ${elapsedSec.toFixed(1)}s.`,
        );
        return result;
      });

      // Step 2: Transcribe pending chunks.
      setStep("transcribe");
      setStepStatus("transcribe", "running");
      addLiveLog(meetingId, "info", "Transcricao iniciada.");
      updatePipelineProgress(
        "transcribe",
        completedAudioSec,
        totalAudioSec,
        completedStoredChunks.length,
        storedChunks.length,
      );

      const newSegments = await transcribeChunksConcurrently({
        chunks: pendingChunks,
        apiKey: keys.groq,
        concurrency: transcriptionConcurrencyForProfile(processingProfile),
        transcribeChunk: async (audioPath, apiKey, offsetSec) => {
          const chunk = pendingChunkByPath.get(audioPath);
          if (chunk) {
            await updateProcessingChunkResult(meetingId, chunk.index, "running");
            patchStoredChunk(chunk.index, { status: "running" });
          }
          try {
            const segments = await transcribeChunk(audioPath, apiKey, offsetSec);
            if (chunk) {
              commitLiveState(meetingId, (state) =>
                appendLiveTranscript(state, chunk.index, segments),
              );
              addLiveLog(meetingId, "success", `Chunk ${chunk.index + 1} transcrito.`);
            }
            const rawSegmentsJson = JSON.stringify(segments);
            if (chunk) {
              segmentJsonByChunk.set(chunk.index, rawSegmentsJson);
              const updatedChunk = patchStoredChunk(chunk.index, {
                status: "done",
                rawSegmentsJson,
                errorMsg: null,
              });
              if (updatedChunk) {
                scheduleFactExtraction(updatedChunk, segments);
              }
              await updateProcessingChunkResult(
                meetingId,
                chunk.index,
                "done",
                rawSegmentsJson,
              );
            }
            return segments;
          } catch (err) {
            if (chunk) {
              addLiveLog(
                meetingId,
                "error",
                `Falha na transcricao do chunk ${chunk.index + 1}: ${formatError(err)}`,
              );
              await updateProcessingChunkResult(
                meetingId,
                chunk.index,
                "error",
                undefined,
                formatError(err),
              );
              patchStoredChunk(chunk.index, { status: "error", errorMsg: formatError(err) });
            }
            throw err;
          }
        },
        onChunkDone: (event) => {
          transcribedAudioSec = completedAudioSec + event.completedAudioSec;
          transcribedChunkCount = completedStoredChunks.length + event.completedChunks;
          updatePipelineProgress(
            "transcribe",
            transcribedAudioSec,
            totalAudioSec,
            transcribedChunkCount,
            storedChunks.length,
          );
        },
      });

      const segments = mergeSortedTranscriptionSegments(completedSegments, newSegments);
      const meetingFactsPromise = closeFactInput();
      setStepStatus("transcribe", "done");
      addLiveLog(meetingId, "success", "Transcricao concluida.");

      // Step 3: Resolve diarization while facts extraction starts.
      setStep("diarize");
      addLiveLog(meetingId, "info", "Alinhando falantes com a transcricao.");
      updatePipelineProgress(
        "diarize",
        totalAudioSec,
        totalAudioSec,
        storedChunks.length,
        storedChunks.length,
      );
      const segmentsJson = JSON.stringify(segments);
      const diarizedPromise = (async () => {
        const speculative = await speakerTurnsPromise;
        if (speculative.engine === "pyannote") {
          setProcessingNote("Pyannote Community-1 ativo nesta reuniao.");
        } else if (speculative.engine === "modern-cpu-chunked") {
          setProcessingNote(
            "CPU moderno em blocos ativo; falantes normalizados pelo numero esperado.",
          );
        } else if (speculative.engine === "modern-cpu") {
          setProcessingNote(
            speculative.fallbackReason
              ? `Chunked indisponivel: ${speculative.fallbackReason} Usando CPU moderno inteiro.`
              : processingProfile === "precision"
                ? "CPU moderno ativo com refinamento seletivo de trechos suspeitos."
                : "Motor CPU moderno ativo nesta reuniao.",
          );
        } else if (speculative.fallbackReason) {
          setProcessingNote(`Pyannote indisponivel: ${speculative.fallbackReason}`);
        }

        if (speculative.turns.length > 0) {
          const speakerTurnsJson = JSON.stringify(speculative.turns);
          try {
            if (processingProfile === "turbo") {
              const aligned = await alignSpeakerTurnsToTranscription(
                segmentsJson,
                speakerTurnsJson,
              );
              setStepStatus("diarize", "done");
              addLiveLog(meetingId, "success", "Falantes alinhados.");
              return aligned;
            }

            const maxRefinementChunks = processingProfile === "precision" ? 3 : 1;
            const refined = await refineDiarizationSelectively(
              segmentsJson,
              speakerTurnsJson,
              exportedChunks,
              {
                expectedSpeakers: diarizationExpectedSpeakers,
                maxRefinementChunks,
              },
            );
            setStepStatus("diarize", "done");
            addLiveLog(meetingId, "success", "Falantes refinados nos trechos suspeitos.");
            return refined;
          } catch {
            // Fall through to the proven end-to-end path if turn alignment fails.
          }
        }

        const fallback = await diarizeTranscriptionEndToEnd(audioOutput, segmentsJson, {
          audioChunks: exportedChunks,
          mode:
            processingProfile === "turbo"
              ? "fast"
              : processingProfile === "precision"
                ? "precise"
                : "auto",
          expectedSpeakers: diarizationExpectedSpeakers,
        });
        setStepStatus("diarize", "done");
        if (fallback.telemetry) {
          addLiveLog(
            meetingId,
            "info",
            `Diarizacao: modo ${fallback.telemetry.requestedMode}, backend ${fallback.telemetry.backendUsed}, ${fallback.telemetry.wallClockSec.toFixed(1)}s.`,
          );
          if (fallback.telemetry.fallbackReason) {
            addLiveLog(meetingId, "warning", `Fallback de diarizacao: ${fallback.telemetry.fallbackReason}`);
          }
        }
        addLiveLog(meetingId, "success", "Diarizacao concluida.");
        return fallback;
      })().catch((err) => {
        setStepStatus("diarize", "error");
        addLiveLog(meetingId, "error", `Falha na diarizacao: ${formatError(err)}`);
        throw err;
      });
      const diarizedWithLiveSpeakersPromise = diarizedPromise.then((diarized) => {
        commitLiveState(meetingId, (state) =>
          applyLiveTranscriptSpeakers(state, diarized.segments),
        );
        return diarized;
      });

      // Step 4: Extract compact facts per chunk without waiting for speaker alignment.
      setStep("generate");
      setStepStatus("generate", "running");
      updatePipelineProgress(
        "extract_facts",
        totalAudioSec,
        totalAudioSec,
        0,
        storedChunks.length,
      );

      factsPhaseVisible = true;
      addLiveLog(meetingId, "info", "Extraindo decisoes, acoes e riscos por chunk.");
      setLiveTab("insights");
      reportFactProgress();

      const [diarized, meetingFacts] = await Promise.all([
        diarizedWithLiveSpeakersPromise,
        meetingFactsPromise,
      ]);
      const diarizedJson = JSON.stringify(diarized);
      const diarizedSpeakersJson = JSON.stringify(diarized.speakers);
      const meetingFactsJson = JSON.stringify(meetingFacts);

      await saveTranscription(
        meetingId,
        segmentsJson,
        diarizedJson,
        diarizedSpeakersJson
      );

      updatePipelineProgress(
        "generate",
        totalAudioSec,
        totalAudioSec,
        storedChunks.length,
        storedChunks.length,
      );
      setLiveTab("minutes");
      const preferLocalMinutes = processingProfile !== "precision";
      addLiveLog(
        meetingId,
        "info",
        preferLocalMinutes
          ? "Montando ata final localmente."
          : "Gerando ata final com streaming.",
      );
      minutesStreamRawSnapshots.set(meetingId, "");
      minutesStreamRawRef.current = "";
      commitLiveState(meetingId, (state) => ({
        ...state,
        minutesDraft: state.minutesDraft || buildLiveMinutesDraft(state.insights, participantNames),
        finalMinutesText: "",
      }));
      const ataHtml = await generateAtaFromFactsStreaming(
        meetingId,
        diarizedJson,
        meetingFactsJson,
        keys.gemini,
        participantNames,
        meetingMetadata,
        preferLocalMinutes,
      );
      minutesStreamRawSnapshots.set(meetingId, ataHtml);
      minutesStreamRawRef.current = ataHtml;
      commitLiveState(meetingId, (state) =>
        setFinalMinutesText(state, htmlToReadablePreview(ataHtml)),
      );
      setStepStatus("generate", "done");
      addLiveLog(meetingId, "success", "Ata final gerada.");

      // Save minutes
      await saveMinutes(meetingId, ataHtml);

      const benchmarkRun = buildBenchmarkRun({
        meetingId,
        title: meeting.title,
        sourcePath: meeting.filePath,
        processingSec: (Date.now() - startedAtRef.current) / 1000,
        audioSec: totalAudioSec,
        engine: `meeting-minutes-local-v1-${processingProfile}`,
        speakers: diarized.speakers,
        facts: meetingFacts,
        mediaMetadata: meetingMetadata,
      });
      const benchmarkRunPath = buildBenchmarkRunArtifactPath(processingWorkDir, meetingId);
      await saveBenchmarkRun(benchmarkRunPath, JSON.stringify(benchmarkRun, null, 2));

      await updateMeetingStatus(meetingId, "done");

      const updatedMeetings = await getMeetings();
      setMeetings(updatedMeetings);
      updatePipelineProgress(
        "complete",
        totalAudioSec,
        totalAudioSec,
        storedChunks.length,
        storedChunks.length,
      );

      if (visibleProcessingMeetingId === meetingId) {
        navigate(`/minutes/${meetingId}`);
      }
      markProcessingSnapshotCompleted(meetingId);
    } catch (err) {
      console.error("Processing pipeline failed:", err);
      const step = useMeetingStore.getState().currentStep;
      if (step) setStepStatus(step, "error");
      addLiveLog(meetingId, "error", formatError(err) || "Erro desconhecido no processamento.");
      setError(formatError(err) || "Erro desconhecido");
      if (id) await updateMeetingStatus(id, "error").catch(() => {});
      markProcessingSnapshotCompleted(meetingId);
    }
  };

  return (
    <div className="mx-auto max-w-5xl space-y-6">
      <header className="flex flex-col gap-3 sm:flex-row sm:items-end sm:justify-between">
        <div>
          <h2 className="text-2xl font-bold text-gray-900">Processando reuniao</h2>
          <p className="mt-2 max-w-2xl text-sm leading-6 text-gray-600">
            Acompanhe o arquivo por etapa. Se voce navegar para outra tela, volte pelo atalho
            de processamento na lateral ou pelo historico.
          </p>
        </div>
        <div className="rounded-lg border border-blue-100 bg-blue-50 px-4 py-3 text-blue-700">
          <p className="text-xs font-medium uppercase tracking-wide">Progresso geral</p>
          <p className="mt-1 text-2xl font-bold tabular-nums">{progress}%</p>
          <p className="mt-1 text-xs font-medium">{PROFILE_LABELS[runProfile]}</p>
        </div>
      </header>
      <ProgressPipeline stepStatus={stepStatus} currentStep={currentStep} />
      {processingNote && (
        <div className="rounded-lg border border-blue-100 bg-blue-50 px-4 py-3 text-sm leading-6 text-blue-900">
          <span className="font-semibold">Motor de falantes: </span>
          {processingNote}
        </div>
      )}
      <div className="rounded-lg border border-gray-200 bg-white p-5 shadow-sm">
        <div className="mb-4 flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
          <div className="min-w-0">
            <p className="text-xs font-semibold uppercase tracking-wide text-blue-600">
              Fase atual
            </p>
            <h3 className="mt-1 text-lg font-semibold text-gray-900">{progressTitle}</h3>
            <p className="mt-2 max-w-2xl text-sm leading-6 text-gray-600">{progressDetail}</p>
          </div>
          <div className="shrink-0 rounded-lg bg-gray-50 px-4 py-3 text-left sm:text-right">
            <p className="text-xs font-medium uppercase tracking-wide text-gray-500">Concluido</p>
            <p className="mt-1 text-3xl font-bold tabular-nums text-blue-600">{progress}%</p>
          </div>
        </div>

        {(progressEta || progressSpeed) && (
          <dl className="mb-4 grid gap-2 sm:grid-cols-2">
            {progressEta && (
              <div className="rounded-lg border border-gray-100 bg-gray-50 px-3 py-2">
                <dt className="text-xs font-medium uppercase tracking-wide text-gray-500">
                  Tempo estimado
                </dt>
                <dd className="mt-1 text-sm font-semibold text-gray-800">{progressEta}</dd>
              </div>
            )}
            {progressSpeed && (
              <div className="rounded-lg border border-gray-100 bg-gray-50 px-3 py-2">
                <dt className="text-xs font-medium uppercase tracking-wide text-gray-500">
                  Velocidade
                </dt>
                <dd className="mt-1 text-sm font-semibold text-gray-800">{progressSpeed}</dd>
              </div>
            )}
          </dl>
        )}

        <div
          className="h-2.5 overflow-hidden rounded-full bg-gray-100"
          role="progressbar"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={progress}
          aria-label="Progresso do processamento"
        >
          <div
            className="h-full rounded-full bg-blue-600 transition-all duration-500 ease-out"
            style={{ width: `${progress}%` }}
          />
        </div>
      </div>
      <section
        aria-label="Painel ao vivo do processamento"
        className="rounded-lg border border-gray-200 bg-white shadow-sm"
      >
        <div className="flex flex-col gap-3 border-b border-gray-100 px-5 py-4 sm:flex-row sm:items-center sm:justify-between">
          <div>
            <p className="text-xs font-semibold uppercase tracking-wide text-blue-600">Ao vivo</p>
            <h3 className="mt-1 text-lg font-semibold text-gray-950">
              Transcricao, insights e ata
            </h3>
          </div>
          <div className="flex flex-wrap gap-2">
            {liveTabItems.map((tab) => {
              const selected = liveTab === tab.key;
              return (
                <button
                  key={tab.key}
                  type="button"
                  aria-pressed={selected}
                  onClick={() => {
                    if (tab.key === "transcript") {
                      transcriptAutoScrollRef.current = false;
                    }
                    setLiveTab(tab.key);
                  }}
                  className={`rounded-lg px-3 py-2 text-sm font-semibold transition ${
                    selected
                      ? "bg-gray-950 text-white shadow-sm"
                      : "bg-gray-50 text-gray-700 hover:bg-gray-100"
                  }`}
                >
                  {tab.label}
                  <span
                    className={`ml-2 rounded-full px-2 py-0.5 text-xs ${
                      selected ? "bg-white/15 text-white" : "bg-white text-gray-500"
                    }`}
                  >
                    {tab.count}
                  </span>
                </button>
              );
            })}
          </div>
        </div>

        <div
          ref={livePanelScrollRef}
          role="region"
          aria-label="Conteudo ao vivo"
          className="max-h-[28rem] overflow-y-auto px-5 py-4"
        >
          {liveTab === "transcript" && (
            <div className="space-y-2.5">
              {liveState.transcript.length === 0 ? (
                <p className="rounded-lg bg-gray-50 px-4 py-6 text-center text-sm text-gray-500">
                  Aguardando primeiro trecho transcrito.
                </p>
              ) : (
                liveState.transcript.map((item) => (
                  <article
                    key={item.id}
                    className="grid gap-3 rounded-lg border border-gray-100 bg-white px-4 py-3 shadow-sm sm:grid-cols-[7rem_1fr]"
                  >
                    <div className="flex items-start gap-2 sm:block">
                      <div className="rounded-md bg-blue-50 px-2.5 py-1 text-xs font-semibold tabular-nums text-blue-700">
                        {item.timeLabel}
                        <span className="mx-1 text-blue-300">-</span>
                        {item.endTimeLabel}
                      </div>
                    </div>
                    <div className="min-w-0">
                      <div className="mb-1 flex flex-wrap items-center gap-2">
                        <span className="text-xs font-semibold uppercase tracking-wide text-gray-500">
                          Bloco {item.chunkIndex + 1}
                        </span>
                        <span className="rounded-full bg-gray-100 px-2 py-0.5 text-xs font-medium text-gray-500">
                          {item.segmentCount === 1 ? "1 fala" : `${item.segmentCount} falas`}
                        </span>
                        <span className="rounded-full bg-amber-50 px-2 py-0.5 text-xs font-medium text-amber-700">
                          {item.speaker}
                        </span>
                      </div>
                      <p className="text-[15px] leading-7 text-gray-900">{item.text}</p>
                    </div>
                  </article>
                ))
              )}
            </div>
          )}

          {liveTab === "insights" && (
            <div className="space-y-3">
              {liveState.insights.length === 0 ? (
                <p className="rounded-lg bg-gray-50 px-4 py-6 text-center text-sm text-gray-500">
                  Aguardando primeiros insights.
                </p>
              ) : (
                liveState.insights.map((item) => (
                  <article
                    key={item.id}
                    className="rounded-lg border border-gray-100 bg-gray-50 px-4 py-3"
                  >
                    <div className="flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between">
                      <div>
                        <p className="text-xs font-semibold uppercase tracking-wide text-blue-600">
                          {item.timeLabel} · Chunk {item.chunkIndex + 1}
                        </p>
                        <p className="mt-1 text-sm leading-6 text-gray-800">{item.summary}</p>
                      </div>
                      <div className="flex shrink-0 flex-wrap gap-2 text-xs font-semibold text-gray-600">
                        <span className="rounded-full bg-white px-2 py-1">
                          {item.decisionCount} decisoes
                        </span>
                        <span className="rounded-full bg-white px-2 py-1">
                          {item.actionCount} acoes
                        </span>
                        <span className="rounded-full bg-white px-2 py-1">
                          {item.riskCount} riscos
                        </span>
                      </div>
                    </div>
                    {item.topics.length > 0 && (
                      <div className="mt-3 flex flex-wrap gap-2">
                        {item.topics.map((topic) => (
                          <span
                            key={topic}
                            className="rounded-full border border-blue-100 bg-blue-50 px-2.5 py-1 text-xs font-medium text-blue-700"
                          >
                            {topic}
                          </span>
                        ))}
                      </div>
                    )}
                    {(item.decisions.length > 0 || item.actions.length > 0) && (
                      <div className="mt-3 grid gap-3 md:grid-cols-2">
                        {item.decisions.length > 0 && (
                          <div>
                            <p className="text-xs font-semibold uppercase tracking-wide text-gray-500">
                              Decisoes
                            </p>
                            <ul className="mt-1 space-y-1 text-sm leading-6 text-gray-800">
                              {item.decisions.slice(0, 3).map((decision) => (
                                <li key={decision}>- {decision}</li>
                              ))}
                            </ul>
                          </div>
                        )}
                        {item.actions.length > 0 && (
                          <div>
                            <p className="text-xs font-semibold uppercase tracking-wide text-gray-500">
                              Acoes
                            </p>
                            <ul className="mt-1 space-y-1 text-sm leading-6 text-gray-800">
                              {item.actions.slice(0, 3).map((action) => (
                                <li key={action}>- {action}</li>
                              ))}
                            </ul>
                          </div>
                        )}
                      </div>
                    )}
                  </article>
                ))
              )}
            </div>
          )}

          {liveTab === "minutes" && (
            <div className="space-y-4">
              {liveState.finalMinutesText ? (
                <pre className="whitespace-pre-wrap rounded-lg border border-blue-100 bg-blue-50 px-4 py-4 text-sm leading-6 text-gray-900">
                  {liveState.finalMinutesText}
                </pre>
              ) : liveState.minutesDraft ? (
                <pre className="whitespace-pre-wrap rounded-lg border border-gray-100 bg-gray-50 px-4 py-4 text-sm leading-6 text-gray-800">
                  {liveState.minutesDraft}
                </pre>
              ) : (
                <p className="rounded-lg bg-gray-50 px-4 py-6 text-center text-sm text-gray-500">
                  Aguardando fatos para montar a ata.
                </p>
              )}
            </div>
          )}

          {liveTab === "logs" && (
            <div className="space-y-2">
              {liveState.logs.length === 0 ? (
                <p className="rounded-lg bg-gray-50 px-4 py-6 text-center text-sm text-gray-500">
                  Aguardando eventos tecnicos.
                </p>
              ) : (
                liveState.logs.map((item) => (
                  <div
                    key={item.id}
                    className="flex gap-3 rounded-lg border border-gray-100 bg-gray-50 px-3 py-2 text-sm"
                  >
                    <span className="w-14 shrink-0 tabular-nums text-gray-500">
                      {item.timeLabel}
                    </span>
                    <span
                      className={`shrink-0 rounded-full px-2 py-0.5 text-xs font-semibold ring-1 ${liveLogBadgeClass(
                        item.level,
                      )}`}
                    >
                      {item.level}
                    </span>
                    <span className="min-w-0 text-gray-800">{item.message}</span>
                  </div>
                ))
              )}
            </div>
          )}
        </div>
      </section>
      {error && (
        <div className="mt-6 p-4 bg-red-50 border border-red-200 rounded-lg">
          <p className="text-sm text-red-700">{error}</p>
          <button
            onClick={() => navigate("/upload")}
            className="mt-3 px-4 py-2 bg-red-600 text-white rounded text-sm hover:bg-red-700"
          >
            Voltar
          </button>
        </div>
      )}
    </div>
  );
}
