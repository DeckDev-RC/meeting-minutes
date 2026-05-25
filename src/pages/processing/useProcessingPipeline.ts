import { useEffect, useMemo, useRef, useState } from "react";
import { listen as tauriListen } from "@tauri-apps/api/event";
import { useParams, useNavigate } from "react-router-dom";
import { htmlToReadablePreview } from "./utils";
import { useMeetingStore } from "../../store/meetingStore";
import { upsertProcessingJob } from "../../lib/tauri";
import { derivePipelineProgress, type PipelinePhase } from "../../lib/pipelineProgress";
import {
  appendLiveLog,
  createLiveProcessingState,
  setFinalMinutesText,
  type LiveLogLevel,
  type LiveProcessingState,
  type LiveTab,
} from "../../lib/liveProcessing";
import type { ProcessingProfile } from "../../lib/types";
import {
  activeProcessingRuns,
  cleanupCompletedLiveProcessingSnapshots,
  completedProcessingSnapshots,
  COMPLETED_SNAPSHOT_CLEANUP_INTERVAL_MS,
  isVisibleProcessingMeeting,
  liveProcessingLastPublishedAt,
  liveProcessingPublishers,
  liveProcessingPublishTimers,
  liveProcessingSnapshots,
  liveProcessingTouchedAt,
  minutesStreamRawSnapshots,
  pendingProcessingStartTimers,
  scheduleLiveStatePublish,
  setVisibleProcessingMeetingId,
} from "./liveSession";
import {
  runProcessingPipeline,
  type PendingPipelineProgress,
} from "./pipelineRunner";
export { PROFILE_LABELS } from "./profiles";

const PROCESSING_JOB_STAGE_ORDER: PipelinePhase[] = [
  "prepare_audio",
  "detect_speech",
  "create_chunks",
  "transcribe",
  "diarize",
  "extract_facts",
  "wait_speakers",
  "generate",
  "complete",
];

type MinutesStreamPayload = {
  meetingId: string;
  delta: string;
  done: boolean;
};

const listenToAppEvent = <T,>(eventName: string, handler: (event: { payload: T }) => void) => {
  const mock = window.__MEETING_MINUTES_E2E__?.listen;
  if (mock) {
    return mock<T>(eventName, handler);
  }

  return tauriListen<T>(eventName, handler);
};

export function useProcessingPipeline() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const startedAtRef = useRef<number>(Date.now());
  const lastProgressRenderRef = useRef<number>(0);
  const livePanelScrollRef = useRef<HTMLDivElement | null>(null);
  const transcriptAutoScrollRef = useRef(true);
  const pendingProgressRef = useRef<PendingPipelineProgress | null>(null);
  const progressTimerRef = useRef<ReturnType<typeof window.setTimeout> | null>(null);
  const minutesStreamRenderTimerRef = useRef<ReturnType<typeof window.setTimeout> | null>(null);
  const minutesStreamRawRef = useRef("");
  const processingJobSnapshotRef = useRef(new Map<string, string>());
  const runningProcessingStagesRef = useRef(new Set<PipelinePhase>());
  const completedProcessingStagesRef = useRef(new Set<PipelinePhase>());
  const [runProfile, setRunProfile] = useState<ProcessingProfile>("balanced");
  const [liveState, setLiveState] = useState<LiveProcessingState>(() =>
    createLiveProcessingState(),
  );
  const [liveTab, setLiveTab] = useState<LiveTab>("transcript");
  const [transcriptionRouteNote, setTranscriptionRouteNote] = useState("");
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

  const recordProcessingJob = (
    meetingId: string,
    stage: string,
    status: "pending" | "running" | "done" | "error",
    progressPct: number,
    errorMsg?: string | null,
  ) => {
    const safeProgress = Math.max(0, Math.min(100, Math.round(progressPct)));
    const key = `${meetingId}:${stage}`;
    const snapshot = `${status}:${safeProgress}:${errorMsg ?? ""}`;
    if (processingJobSnapshotRef.current.get(key) === snapshot) {
      return;
    }
    processingJobSnapshotRef.current.set(key, snapshot);
    void upsertProcessingJob(meetingId, stage, status, safeProgress, errorMsg).catch((err) => {
      console.warn("Failed to persist processing job:", err);
    });
  };

  const closeProcessingStage = (meetingId: string, stage: PipelinePhase) => {
    if (completedProcessingStagesRef.current.has(stage)) return;
    completedProcessingStagesRef.current.add(stage);
    runningProcessingStagesRef.current.delete(stage);
    recordProcessingJob(meetingId, stage, "done", 100);
  };

  const closePreviousProcessingStages = (meetingId: string, phase: PipelinePhase) => {
    if (phase === "complete") {
      for (const stage of Array.from(runningProcessingStagesRef.current)) {
        closeProcessingStage(meetingId, stage);
      }
      return;
    }

    const phaseIndex = PROCESSING_JOB_STAGE_ORDER.indexOf(phase);
    for (const stage of Array.from(runningProcessingStagesRef.current)) {
      const stageIndex = PROCESSING_JOB_STAGE_ORDER.indexOf(stage);
      if (stageIndex >= 0 && stageIndex < phaseIndex) {
        closeProcessingStage(meetingId, stage);
      }
    }
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
    if (id) {
      closePreviousProcessingStages(id, phase);
      if (phase === "complete") {
        closeProcessingStage(id, phase);
      } else if (!completedProcessingStagesRef.current.has(phase)) {
        runningProcessingStagesRef.current.add(phase);
        recordProcessingJob(id, phase, "running", view.percent);
      }
    }
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
  const liveInsightTotals = useMemo(
    () => ({
      decisions: liveState.insights.reduce((sum, item) => sum + item.decisionCount, 0),
      actions: liveState.insights.reduce((sum, item) => sum + item.actionCount, 0),
      risks: liveState.insights.reduce((sum, item) => sum + item.riskCount, 0),
    }),
    [liveState.insights],
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
    setVisibleProcessingMeetingId(id);
    setCurrentMeeting(id);
    const snapshot = liveProcessingSnapshots.get(id) ?? createLiveProcessingState();
    liveProcessingSnapshots.set(id, snapshot);
    liveProcessingTouchedAt.set(id, Date.now());
    liveProcessingPublishers.set(id, setLiveState);
    setLiveState(snapshot);
    liveProcessingLastPublishedAt.set(id, Date.now());
    processingJobSnapshotRef.current.clear();
    runningProcessingStagesRef.current.clear();
    completedProcessingStagesRef.current.clear();
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
      if (isVisibleProcessingMeeting(id)) {
        setVisibleProcessingMeetingId(null);
      }
    };
  }, [id]);

  const runPipeline = (meetingId: string) =>
    runProcessingPipeline(meetingId, {
      activeMeetingId: id,
      progress,
      navigate,
      startedAtRef,
      lastProgressRenderRef,
      pendingProgressRef,
      progressTimerRef,
      minutesStreamRenderTimerRef,
      minutesStreamRawRef,
      transcriptAutoScrollRef,
      setRunProfile,
      setLiveState,
      setLiveTab,
      setTranscriptionRouteNote,
      setCurrentMeeting,
      setStep,
      setStepStatus,
      setProcessingNote,
      setError,
      setMeetings,
      addLiveLog,
      commitLiveState,
      recordProcessingJob,
      updatePipelineProgress,
    });
  const visibleProcessingNotes = [transcriptionRouteNote, processingNote].filter(
    (note, index, notes): note is string => Boolean(note) && notes.indexOf(note) === index,
  );

  const handleSelectLiveTab = (tab: LiveTab) => {
    if (tab === "transcript") {
      transcriptAutoScrollRef.current = false;
    }
    setLiveTab(tab);
  };

  return {
    runProfile,
    stepStatus,
    currentStep,
    progress,
    progressTitle,
    progressDetail,
    progressEta,
    progressSpeed,
    liveState,
    liveTab,
    liveTabItems,
    livePanelScrollRef,
    transcriptCount,
    insightCount,
    logCount,
    liveInsightTotals,
    visibleProcessingNotes,
    error,
    onSelectLiveTab: handleSelectLiveTab,
    onBackToUpload: () => navigate("/upload"),
  };
}

