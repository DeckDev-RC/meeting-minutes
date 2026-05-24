import type { MutableRefObject } from "react";
import {
  chunkOverlapForProfile,
  formatError,
  htmlToReadablePreview,
  joinPath,
  normalizeProcessingProfile,
  parseParticipantsHint,
  parseStoredSegments,
  sumChunkDurations,
  toExportedChunk,
  toNewProcessingChunkRecord,
} from "./utils";
import {
  alignSpeakerTurnsToTranscription,
  diarizeTranscriptionEndToEnd,
  generateAtaFromFactsStreaming,
  getApiKeys,
  getMeetings,
  getProcessingChunks,
  prepareAudioAndChunks,
  probeMediaMetadata,
  refineDiarizationSelectively,
  resolveProcessingWorkDir,
  saveBenchmarkRun,
  saveMinutes,
  saveProcessingChunks,
  saveTranscription,
  updateMeetingStatus,
} from "../../lib/tauri";
import { mergeSortedTranscriptionSegments } from "../../lib/segmentMerge";
import { buildBenchmarkRun, buildBenchmarkRunArtifactPath } from "../../lib/benchmarkRun";
import {
  buildDiarizationPlan,
  resolveDiarizationExpectedSpeakers,
} from "../../lib/speakerCount";
import {
  chunkOutputFormatForSpeakerRuntime,
  normalizeSpeakerDiarizationRuntime,
} from "../../lib/diarizationRuntime";
import {
  applyLiveTranscriptSpeakers,
  appendLiveTranscript,
  buildLiveMinutesDraft,
  createLiveProcessingState,
  setFinalMinutesText,
  type LiveLogLevel,
  type LiveProcessingState,
  type LiveTab,
} from "../../lib/liveProcessing";
import type {
  DiarizedResult,
  JobStep,
  Meeting,
  ProcessingChunkRecord,
  ProcessingProfile,
} from "../../lib/types";
import { useMeetingStore } from "../../store/meetingStore";
import {
  completedProcessingSnapshots,
  isVisibleProcessingMeeting,
  liveProcessingLastPublishedAt,
  liveProcessingParticipantNames,
  liveProcessingSnapshots,
  markProcessingSnapshotCompleted,
  minutesStreamRawSnapshots,
} from "./liveSession";
import { createFactExtractionQueue } from "./factExtractionQueue";
import { PROFILE_LABELS } from "./profiles";
import { startSpeculativeSpeakerTurns } from "./speakerDiarization";
import { transcribePendingChunks } from "./transcriptionStage";

export type PendingPipelineProgress = {
  percent: number;
  title: string;
  detail: string;
  etaLabel: string;
  speedLabel: string;
};

type ProcessingStepStatus = "pending" | "running" | "done" | "error";

type ProcessingPipelineRunnerContext = {
  activeMeetingId?: string;
  progress: number;
  navigate: (path: string) => void;
  startedAtRef: MutableRefObject<number>;
  lastProgressRenderRef: MutableRefObject<number>;
  pendingProgressRef: MutableRefObject<PendingPipelineProgress | null>;
  progressTimerRef: MutableRefObject<ReturnType<typeof window.setTimeout> | null>;
  minutesStreamRenderTimerRef: MutableRefObject<ReturnType<typeof window.setTimeout> | null>;
  minutesStreamRawRef: MutableRefObject<string>;
  transcriptAutoScrollRef: MutableRefObject<boolean>;
  setRunProfile: (profile: ProcessingProfile) => void;
  setLiveState: (state: LiveProcessingState) => void;
  setLiveTab: (tab: LiveTab) => void;
  setTranscriptionRouteNote: (note: string) => void;
  setCurrentMeeting: (meetingId: string) => void;
  setStep: (step: JobStep) => void;
  setStepStatus: (step: JobStep, status: ProcessingStepStatus) => void;
  setProcessingNote: (note: string) => void;
  setError: (error: string | null) => void;
  setMeetings: (meetings: Meeting[]) => void;
  addLiveLog: (meetingId: string, level: LiveLogLevel, message: string) => void;
  commitLiveState: (
    meetingId: string,
    updater: (state: LiveProcessingState) => LiveProcessingState,
  ) => void;
  recordProcessingJob: (
    meetingId: string,
    stage: string,
    status: ProcessingStepStatus,
    progressPct: number,
    errorMsg?: string | null,
  ) => void;
  updatePipelineProgress: (
    phase: import("../../lib/pipelineProgress").PipelinePhase,
    completedAudioSec: number,
    totalAudioSec: number,
    completedChunks: number,
    totalChunks: number,
  ) => void;
};
export const runProcessingPipeline = async (
  meetingId: string,
  context: ProcessingPipelineRunnerContext,
) => {
  const {
    activeMeetingId,
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
  } = context;
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
      setTranscriptionRouteNote("");
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
      if (!keys.gemini) {
        setError("Configure a chave Gemini em Configuracoes antes de processar.");
        return;
      }

      const meetings = await getMeetings();
      const meeting = meetings.find((m) => m.id === meetingId);
      if (!meeting) {
        setError("Reuniao nao encontrada.");
        return;
      }
      const processingProfile = normalizeProcessingProfile(meeting.processingProfile);
      const meetingTranscriptionProfile =
        meeting.transcriptionProfile ?? keys.transcriptionProfile ?? "smart-low-cost";
      const speakerDiarizationRuntime = normalizeSpeakerDiarizationRuntime(
        keys.speakerDiarizationRuntime,
      );
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
          ? "Precisao: estrategia de falantes definida apos preparar os blocos; depois refina trechos suspeitos."
          : processingProfile === "turbo"
            ? "Turbo: chunks sem overlap para exportacao em lote, transcricao mais concorrente e sem refinamento seletivo."
            : "Motor CPU moderno ativo para identificar falantes em paralelo.";
      setRunProfile(processingProfile);
      setProcessingNote(speakerProcessingNote);
      addLiveLog(meetingId, "info", `Perfil ${PROFILE_LABELS[processingProfile]} selecionado.`);
      addLiveLog(
        meetingId,
        "info",
        speakerDiarizationRuntime === "modern-cpu"
          ? "Motor de falantes: CPU moderno."
          : `Motor de falantes: ${speakerDiarizationRuntime}.`,
      );
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
        if (isVisibleProcessingMeeting(meetingId)) {
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
          outputFormat: chunkOutputFormatForSpeakerRuntime(speakerDiarizationRuntime),
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
      const pendingChunks = storedChunks.flatMap((chunk, index) =>
        chunk.status !== "done" ? [exportedChunks[index]] : [],
      );
      const pendingChunkByPath = new Map(pendingChunks.map((chunk) => [chunk.audioPath, chunk]));
      const factQueue = createFactExtractionQueue({
        meetingId,
        storedChunks,
        totalAudioSec,
        processingProfile,
        geminiApiKey: keys.gemini,
        participantNames,
        parsedSegmentsByChunk,
        segmentJsonByChunk,
        commitLiveState,
        addLiveLog,
        updatePipelineProgress,
        patchStoredChunk,
      });
      for (const chunk of storedChunks) {
        factQueue.scheduleChunk(chunk);
      }
      setStepStatus("extract_audio", "done");
      setStepStatus("diarize", "running");
      addLiveLog(meetingId, "info", "Identificacao de falantes iniciada em paralelo.");
      const diarizationPlan = buildDiarizationPlan({
        expectedSpeakers: diarizationExpectedSpeakers,
        audioChunkCount: exportedChunks.length,
        totalAudioSec,
      });
      if (diarizationPlan.preferChunked) {
        addLiveLog(
          meetingId,
          "info",
          diarizationPlan.strategy === "chunked-auto"
            ? "Diarizacao CPU em blocos automaticos ativada para audio longo sem numero esperado de falantes."
            : "Diarizacao CPU em blocos ativada para esta reuniao.",
        );
        if (diarizationPlan.warning) {
          addLiveLog(meetingId, "warning", diarizationPlan.warning);
        }
        setProcessingNote(diarizationPlan.note);
      }
      const speakerTurnsStartedAt = Date.now();
      const speakerTurnsPromise = startSpeculativeSpeakerTurns(
        audioOutput,
        diarizationExpectedSpeakers,
        exportedChunks,
        diarizationPlan.preferChunked,
        false,
        speakerDiarizationRuntime,
      ).then((result) => {
        const elapsedSec = (Date.now() - speakerTurnsStartedAt) / 1000;
        addLiveLog(
          meetingId,
          result.error ? "warning" : "success",
          `Motor de falantes ${result.engine} concluiu em ${elapsedSec.toFixed(1)}s.`,
        );
        return result;
      });

      const newSegments = await transcribePendingChunks({
        meetingId,
        keys,
        meetingTranscriptionProfile,
        totalAudioSec,
        processingProfile,
        pendingChunks,
        pendingChunkByPath,
        completedAudioSec,
        completedStoredChunks,
        storedChunks,
        segmentJsonByChunk,
        factQueue,
        setStep,
        setStepStatus,
        setProcessingNote,
        setTranscriptionRouteNote,
        addLiveLog,
        commitLiveState,
        patchStoredChunk,
        updatePipelineProgress,
      });
      const segments = mergeSortedTranscriptionSegments(completedSegments, newSegments);
      const meetingFactsPromise = factQueue.closeInput();
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
        if (speculative.engine === "sherpa-onnx") {
          setProcessingNote(
            speakerDiarizationRuntime === "sherpa-onnx-cuda"
              ? "ONNX/sherpa ativo com tentativa CUDA e fallback automatico."
              : "ONNX/sherpa CPU ativo para identificar falantes em blocos.",
          );
        } else if (speculative.engine === "pyannote") {
          setProcessingNote("Pyannote Community-1 ativo nesta reuniao.");
        } else if (speculative.engine === "modern-cpu-chunked") {
          setProcessingNote(
            diarizationPlan.strategy === "chunked-auto"
              ? "CPU moderno em blocos automatico; falantes detectados e costurados por centroides."
              : "CPU moderno em blocos ativo; falantes normalizados pelo numero esperado.",
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

        const warnIfAutoChunkedFragmented = (result: DiarizedResult) => {
          if (diarizationPlan.strategy !== "chunked-auto" || result.speakers.length <= 12) {
            return;
          }
          addLiveLog(
            meetingId,
            "warning",
            `Diarizacao automatica encontrou ${result.speakers.length} rotulos de falantes; informe o numero esperado para reduzir fragmentacao.`,
          );
        };

        if (speculative.turns.length > 0) {
          const speakerTurnsJson = JSON.stringify(speculative.turns);
          try {
            if (processingProfile === "turbo") {
              const aligned = await alignSpeakerTurnsToTranscription(
                segmentsJson,
                speakerTurnsJson,
              );
              warnIfAutoChunkedFragmented(aligned);
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
            warnIfAutoChunkedFragmented(refined);
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
        warnIfAutoChunkedFragmented(fallback);
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
      let diarizationComplete = false;
      let waitingForSpeakersTimer: ReturnType<typeof window.setInterval> | null = null;
      const stopWaitingForSpeakersProgress = () => {
        if (waitingForSpeakersTimer) {
          window.clearInterval(waitingForSpeakersTimer);
          waitingForSpeakersTimer = null;
        }
      };
      const startWaitingForSpeakersProgress = () => {
        if (waitingForSpeakersTimer || diarizationComplete) return;
        addLiveLog(
          meetingId,
          "info",
          "Insights prontos; aguardando a identificacao de falantes terminar.",
        );
        updatePipelineProgress(
          "wait_speakers",
          totalAudioSec,
          totalAudioSec,
          storedChunks.length,
          storedChunks.length,
        );
        waitingForSpeakersTimer = window.setInterval(() => {
          updatePipelineProgress(
            "wait_speakers",
            totalAudioSec,
            totalAudioSec,
            storedChunks.length,
            storedChunks.length,
          );
        }, 5000);
      };
      const diarizedWithLiveSpeakersPromise = diarizedPromise.then((diarized) => {
        commitLiveState(meetingId, (state) =>
          applyLiveTranscriptSpeakers(state, diarized.segments),
        );
        return diarized;
      }).finally(() => {
        diarizationComplete = true;
        stopWaitingForSpeakersProgress();
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

      factQueue.setPhaseVisible(true);
      addLiveLog(meetingId, "info", "Extraindo decisoes, acoes e riscos por chunk.");
      setLiveTab("insights");
      factQueue.reportProgress();

      const meetingFactsWithWaitPromise = meetingFactsPromise.then((facts) => {
        if (!diarizationComplete) {
          startWaitingForSpeakersProgress();
        }
        return facts;
      });
      const [diarized, meetingFacts] = await Promise.all([
        diarizedWithLiveSpeakersPromise,
        meetingFactsWithWaitPromise,
      ]).finally(stopWaitingForSpeakersProgress);
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

      // Save minutes plus the structured facts used to build the document.
      await saveMinutes(
        meetingId,
        ataHtml,
        undefined,
        meetingFactsJson,
        diarizedJson,
        participantNames,
      );

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

      if (isVisibleProcessingMeeting(meetingId)) {
        navigate(`/minutes/${meetingId}`);
      }
      markProcessingSnapshotCompleted(meetingId);
    } catch (err) {
      console.error("Processing pipeline failed:", err);
      const step = useMeetingStore.getState().currentStep;
      if (step) setStepStatus(step, "error");
      addLiveLog(meetingId, "error", formatError(err) || "Erro desconhecido no processamento.");
      setError(formatError(err) || "Erro desconhecido");
      recordProcessingJob(
        meetingId,
        "pipeline",
        "error",
        progress,
        formatError(err) || "Erro desconhecido no processamento.",
      );
      if (activeMeetingId) await updateMeetingStatus(activeMeetingId, "error").catch(() => {});
      markProcessingSnapshotCompleted(meetingId);
    }
  };




