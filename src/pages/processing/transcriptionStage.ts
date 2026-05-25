import { getCloudflareQuotaState, markCloudflareQuotaExhausted } from "../../lib/cloudTranscriptionHealth";
import { transcribeChunksWithLocalBackend } from "../../lib/localTranscription";
import { transcriptionConcurrencyForProfile } from "../../lib/processingConcurrency";
import {
  LOCAL_TRANSCRIPTION_REQUIRED_AUDIO_SEC,
  isLocalTranscriptionBackend,
  isQuotaOrRateLimitError,
  selectFallbackTranscriptionBackends,
  selectTranscriptionBackend,
  transcriptionBackendLabel,
  type TranscriptionBackend,
} from "../../lib/transcriptionProvider";
import { scoreCloudflareTranscriptRisk } from "../../lib/transcriptionQuality";
import { transcribeChunksConcurrently } from "../../lib/transcriptionQueue";
import {
  appendLiveTranscript,
  type LiveProcessingState,
} from "../../lib/liveProcessing";
import {
  checkLocalTranscriptionBackends,
  transcribeChunk,
  transcribeChunkCloudflare,
  transcribeChunkDeepgram,
  transcribeChunkLocal,
  updateProcessingChunkResult,
} from "../../lib/tauri";
import type { PipelinePhase } from "../../lib/pipelineProgress";
import type {
  ExportedChunk,
  JobStep,
  ProcessingChunkRecord,
  ProcessingProfile,
  TranscriptionRoutingProfile,
  TranscriptionSegment,
} from "../../lib/types";
import { formatError } from "./utils";
import type { FactExtractionQueue } from "./factExtractionQueue";

type ApiKeys = {
  groq: string;
  cloudflareAccountId: string;
  cloudflareApiToken: string;
  deepgramApiKey: string;
  manualTranscriptionProvider?: TranscriptionBackend;
};

type ProcessingStepStatus = "pending" | "running" | "done" | "error";

type TranscriptionStageContext = {
  meetingId: string;
  keys: ApiKeys;
  meetingTranscriptionProfile: TranscriptionRoutingProfile;
  totalAudioSec: number;
  processingProfile: ProcessingProfile;
  pendingChunks: ExportedChunk[];
  pendingChunkByPath: Map<string, ExportedChunk>;
  completedAudioSec: number;
  completedStoredChunks: ProcessingChunkRecord[];
  storedChunks: ProcessingChunkRecord[];
  segmentJsonByChunk: Map<number, string>;
  factQueue: FactExtractionQueue;
  commitLiveState: (
    meetingId: string,
    updater: (state: LiveProcessingState) => LiveProcessingState,
  ) => void;
  setStep: (step: JobStep) => void;
  setStepStatus: (step: JobStep, status: ProcessingStepStatus) => void;
  setProcessingNote: (note: string) => void;
  setTranscriptionRouteNote: (note: string) => void;
  addLiveLog: (
    meetingId: string,
    level: "info" | "success" | "warning" | "error",
    message: string,
  ) => void;
  patchStoredChunk: (
    chunkIndex: number,
    patch: Partial<ProcessingChunkRecord>,
  ) => ProcessingChunkRecord | undefined;
  updatePipelineProgress: (
    phase: PipelinePhase,
    completedAudioSec: number,
    totalAudioSec: number,
    completedChunks: number,
    totalChunks: number,
  ) => void;
};

export const transcribePendingChunks = async ({
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
  commitLiveState,
  setStep,
  setStepStatus,
  setProcessingNote,
  setTranscriptionRouteNote,
  addLiveLog,
  patchStoredChunk,
  updatePipelineProgress,
}: TranscriptionStageContext): Promise<TranscriptionSegment[]> => {
  let transcribedAudioSec = completedAudioSec;
  let transcribedChunkCount = completedStoredChunks.length;

  // Step 2: Transcribe pending chunks.
  setStep("transcribe");
  setStepStatus("transcribe", "running");
  const localTranscriptionStatus = await checkLocalTranscriptionBackends().catch(() => ({
    fasterWhisperAvailable: false,
    parakeetAvailable: false,
  }));
  const localAvailability = {
    localBackendAvailable: localTranscriptionStatus.fasterWhisperAvailable,
    parakeetBackendAvailable: localTranscriptionStatus.parakeetAvailable,
  };
  const transcriptionBackend = selectTranscriptionBackend({
        totalAudioSec,
        groqApiKey: keys.groq,
        cloudflareAccountId: keys.cloudflareAccountId,
        cloudflareApiToken: keys.cloudflareApiToken,
        deepgramApiKey: keys.deepgramApiKey,
        ...localAvailability,
        profile: meetingTranscriptionProfile,
        manualProvider: keys.manualTranscriptionProvider,
        unavailableBackends: getCloudflareQuotaState(window.localStorage).isExhaustedToday
          ? ["cloudflare"]
          : [],
      });
      addLiveLog(
        meetingId,
        "info",
        `Transcricao iniciada com ${transcriptionBackendLabel(transcriptionBackend)}.`,
      );
      const selectiveDeepgramCorrection =
        transcriptionBackend === "cloudflare" &&
        meetingTranscriptionProfile === "smart-low-cost" &&
        Boolean(keys.deepgramApiKey?.trim());
      const activeTranscriptionRouteNote = selectiveDeepgramCorrection
        ? "Transcricao: Cloudflare com correcao Deepgram seletiva."
        : `Transcricao: ${transcriptionBackendLabel(transcriptionBackend)}.`;
      setTranscriptionRouteNote(activeTranscriptionRouteNote);
      setProcessingNote(activeTranscriptionRouteNote);
      if (
        isLocalTranscriptionBackend(transcriptionBackend) &&
        totalAudioSec >= LOCAL_TRANSCRIPTION_REQUIRED_AUDIO_SEC
      ) {
        addLiveLog(
          meetingId,
          "info",
          "Rota local selecionada para manter processamento sem API de transcricao.",
        );
      }
      const unavailableTranscriptionBackends = new Set<TranscriptionBackend>();
      if (getCloudflareQuotaState(window.localStorage).isExhaustedToday) {
        unavailableTranscriptionBackends.add("cloudflare");
        addLiveLog(
          meetingId,
          "warning",
          "Cloudflare ja estava marcado como sem cota hoje; iniciando direto no fallback.",
        );
      }
      const markTranscriptionBackendUnavailable = (
        backend: TranscriptionBackend,
        reason: string,
      ) => {
        if (isLocalTranscriptionBackend(backend) || unavailableTranscriptionBackends.has(backend)) {
          return;
        }

        unavailableTranscriptionBackends.add(backend);
        if (backend === "cloudflare" && isQuotaOrRateLimitError(reason)) {
          markCloudflareQuotaExhausted(window.localStorage, new Date(), reason);
        }
        const fallbackBackends = selectFallbackTranscriptionBackends({
          totalAudioSec,
          groqApiKey: keys.groq,
          cloudflareAccountId: keys.cloudflareAccountId,
          cloudflareApiToken: keys.cloudflareApiToken,
          deepgramApiKey: keys.deepgramApiKey,
          ...localAvailability,
          profile: meetingTranscriptionProfile,
          manualProvider: keys.manualTranscriptionProvider,
          primaryBackend: backend,
          unavailableBackends: Array.from(unavailableTranscriptionBackends),
        });
        const fallbackLabel = fallbackBackends[0]
          ? transcriptionBackendLabel(fallbackBackends[0])
          : "sem fallback configurado";
        const routeNote = `Transcricao: ${transcriptionBackendLabel(
          backend,
        )} indisponivel nesta execucao; usando ${fallbackLabel} como fallback.`;
        addLiveLog(
          meetingId,
          "warning",
          `${transcriptionBackendLabel(backend)} indisponivel: ${reason}`,
        );
        setTranscriptionRouteNote(routeNote);
        setProcessingNote(routeNote);
      };
      const transcribeWithBackend = async (
        backend: TranscriptionBackend,
        audioPath: string,
        offsetSec: number,
      ) => {
        if (backend === "cloudflare") {
          return transcribeChunkCloudflare(
            audioPath,
            keys.cloudflareAccountId,
            keys.cloudflareApiToken,
            offsetSec,
          );
        }
        if (backend === "deepgram") {
          return transcribeChunkDeepgram(audioPath, keys.deepgramApiKey, offsetSec);
        }
        if (backend === "groq") {
          return transcribeChunk(audioPath, keys.groq, offsetSec);
        }
        return transcribeChunkLocal(audioPath, offsetSec, "turbo");
      };
      const transcribeWithFallbacks = async (
        backend: TranscriptionBackend,
        audioPath: string,
        offsetSec: number,
        chunkIndex?: number,
      ): Promise<{ backend: TranscriptionBackend; segments: TranscriptionSegment[] }> => {
        const primaryCandidates = unavailableTranscriptionBackends.has(backend) ? [] : [backend];
        const fallbackCandidates = selectFallbackTranscriptionBackends({
          totalAudioSec,
          groqApiKey: keys.groq,
          cloudflareAccountId: keys.cloudflareAccountId,
          cloudflareApiToken: keys.cloudflareApiToken,
          deepgramApiKey: keys.deepgramApiKey,
          ...localAvailability,
          profile: meetingTranscriptionProfile,
          manualProvider: keys.manualTranscriptionProvider,
          primaryBackend: backend,
          unavailableBackends: Array.from(unavailableTranscriptionBackends),
        });
        const candidates = [...primaryCandidates, ...fallbackCandidates].filter(
          (candidate, index, all) => all.indexOf(candidate) === index,
        );
        let lastError: unknown = null;

        for (const candidate of candidates) {
          try {
            if (candidate !== backend) {
              addLiveLog(
                meetingId,
                "warning",
                `Chunk ${chunkIndex !== undefined ? chunkIndex + 1 : "atual"} usando fallback ${transcriptionBackendLabel(
                  candidate,
                )}.`,
              );
            }
            return {
              backend: candidate,
              segments: await transcribeWithBackend(candidate, audioPath, offsetSec),
            };
          } catch (err) {
            const message = formatError(err);
            lastError = err;
            if (isQuotaOrRateLimitError(message)) {
              markTranscriptionBackendUnavailable(candidate, message);
            } else if (candidate !== backend) {
              addLiveLog(
                meetingId,
                "warning",
                `Fallback ${transcriptionBackendLabel(candidate)} falhou: ${message}`,
              );
            }
          }
        }

        throw lastError ?? new Error("Nenhum backend de transcricao disponivel.");
      };
      updatePipelineProgress(
        "transcribe",
        completedAudioSec,
        totalAudioSec,
        completedStoredChunks.length,
        storedChunks.length,
      );
      if (completedStoredChunks.length > 0 && pendingChunks.length > 0) {
        const nextChunk = Math.min(...pendingChunks.map((chunk) => chunk.index)) + 1;
        addLiveLog(
          meetingId,
          "info",
          `Retomando do chunk ${nextChunk}/${storedChunks.length} usando ${transcriptionBackendLabel(transcriptionBackend)}.`,
        );
      } else if (completedStoredChunks.length > 0) {
        addLiveLog(
          meetingId,
          "info",
          `Retomada detectada: ${completedStoredChunks.length}/${storedChunks.length} chunks ja estavam transcritos.`,
        );
      }

      let newSegments: TranscriptionSegment[] = [];
      if (isLocalTranscriptionBackend(transcriptionBackend)) {
        for (const chunk of pendingChunks) {
          await updateProcessingChunkResult(meetingId, chunk.index, "running").catch(() => {});
          patchStoredChunk(chunk.index, { status: "running" });
        }
        let localResults: Awaited<ReturnType<typeof transcribeChunksWithLocalBackend>>;
        try {
          localResults = await transcribeChunksWithLocalBackend(transcriptionBackend, pendingChunks);
        } catch (err) {
          const message = formatError(err);
          await Promise.all(
            pendingChunks.map((chunk) =>
              updateProcessingChunkResult(
                meetingId,
                chunk.index,
                "error",
                undefined,
                message,
              ).catch(() => {}),
            ),
          );
          for (const chunk of pendingChunks) {
            patchStoredChunk(chunk.index, { status: "error", errorMsg: message });
          }
          addLiveLog(meetingId, "error", `Falha na transcricao local: ${message}`);
          throw err;
        }
        const localSegmentsByChunk = new Map(
          localResults.map((result) => [result.index, result.segments]),
        );
        const collectedSegments: TranscriptionSegment[] = [];
        for (const chunk of pendingChunks) {
          const segments = localSegmentsByChunk.get(chunk.index) ?? [];
          collectedSegments.push(...segments);
          commitLiveState(meetingId, (state) =>
            appendLiveTranscript(state, chunk.index, segments),
          );
          addLiveLog(meetingId, "success", `Chunk ${chunk.index + 1} transcrito localmente.`);
          const rawSegmentsJson = JSON.stringify(segments);
          segmentJsonByChunk.set(chunk.index, rawSegmentsJson);
          const updatedChunk = patchStoredChunk(chunk.index, {
            status: "done",
            rawSegmentsJson,
            errorMsg: null,
          });
          if (updatedChunk) {
            factQueue.scheduleChunk(updatedChunk, segments);
          }
          await updateProcessingChunkResult(
            meetingId,
            chunk.index,
            "done",
            rawSegmentsJson,
          );
          transcribedAudioSec += chunk.durationSec;
          transcribedChunkCount += 1;
          updatePipelineProgress(
            "transcribe",
            transcribedAudioSec,
            totalAudioSec,
            transcribedChunkCount,
            storedChunks.length,
          );
        }
        newSegments = collectedSegments;
      } else {
        const remoteApiKey =
          transcriptionBackend === "deepgram"
            ? keys.deepgramApiKey
            : transcriptionBackend === "cloudflare"
              ? keys.cloudflareApiToken
              : keys.groq;
        newSegments = await transcribeChunksConcurrently({
          chunks: pendingChunks,
          apiKey: remoteApiKey,
          concurrency: transcriptionConcurrencyForProfile(processingProfile),
          transcribeChunk: async (audioPath, _apiKey, offsetSec, isCancelled) => {
            const chunk = pendingChunkByPath.get(audioPath);
            if (isCancelled?.()) {
              return [];
            }
            if (chunk) {
              await updateProcessingChunkResult(meetingId, chunk.index, "running");
              patchStoredChunk(chunk.index, { status: "running" });
            }
            try {
              const transcriptionResult = await transcribeWithFallbacks(
                transcriptionBackend,
                audioPath,
                offsetSec,
                chunk?.index,
              );
              if (isCancelled?.()) {
                return transcriptionResult.segments;
              }
              let segments = transcriptionResult.segments;
              let segmentBackend = transcriptionResult.backend;
              if (
                segmentBackend === "cloudflare" &&
                meetingTranscriptionProfile === "smart-low-cost" &&
                keys.deepgramApiKey?.trim() &&
                chunk
              ) {
                const risk = scoreCloudflareTranscriptRisk({
                  durationSec: chunk.durationSec,
                  segments,
                });
                if (risk.shouldEscalate) {
                  addLiveLog(
                    meetingId,
                    "warning",
                    `Chunk ${chunk.index + 1} enviado para correcao Deepgram: ${risk.reasons.join(", ")}.`,
                  );
                  try {
                    segments = await transcribeChunkDeepgram(
                      audioPath,
                      keys.deepgramApiKey,
                      offsetSec,
                    );
                    segmentBackend = "deepgram";
                    addLiveLog(
                      meetingId,
                      "success",
                      `Chunk ${chunk.index + 1} corrigido com Deepgram.`,
                    );
                  } catch (correctionError) {
                    addLiveLog(
                      meetingId,
                      "warning",
                      `Correcao Deepgram falhou no chunk ${chunk.index + 1}; mantendo Cloudflare: ${formatError(correctionError)}`,
                    );
                  }
                }
              }
              if (isCancelled?.()) {
                return segments;
              }
              if (chunk) {
                commitLiveState(meetingId, (state) =>
                  appendLiveTranscript(state, chunk.index, segments),
                );
                addLiveLog(
                  meetingId,
                  "success",
                  `Chunk ${chunk.index + 1} transcrito com ${transcriptionBackendLabel(segmentBackend)}.`,
                );
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
                  factQueue.scheduleChunk(updatedChunk, segments);
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
              if (isCancelled?.()) {
                throw err;
              }
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
      }


  return newSegments;
};
