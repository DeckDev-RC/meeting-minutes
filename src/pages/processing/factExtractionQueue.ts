import { appendLiveInsights, type LiveProcessingState } from "../../lib/liveProcessing";
import { buildAdaptiveFactBatches, type FactBatchItem } from "../../lib/meetingFactsQueue";
import { resolveSegmentsForFactScheduling } from "../../lib/processingChunks";
import { factConcurrencyForPhase } from "../../lib/processingConcurrency";
import { extractChunkFacts, extractFactBatch, updateProcessingChunkFacts } from "../../lib/tauri";
import type {
  MeetingChunkInsights,
  ProcessingChunkRecord,
  ProcessingProfile,
  TranscriptionSegment,
} from "../../lib/types";
import {
  formatError,
  parseCachedFacts,
  parseStoredSegments,
} from "./utils";

type FactQueueItem = {
  chunk: ProcessingChunkRecord;
  segments: TranscriptionSegment[];
  segmentsJson: string;
};

type CommitLiveState = (
  meetingId: string,
  updater: (state: LiveProcessingState) => LiveProcessingState,
) => void;

type AddLiveLog = (
  meetingId: string,
  level: "info" | "success" | "warning" | "error",
  message: string,
) => void;

type UpdatePipelineProgress = (
  phase: "extract_facts",
  completedAudioSec: number,
  totalAudioSec: number,
  completedChunks: number,
  totalChunks: number,
) => void;

type PatchStoredChunk = (
  chunkIndex: number,
  patch: Partial<ProcessingChunkRecord>,
) => ProcessingChunkRecord | undefined;

type FactExtractionQueueOptions = {
  meetingId: string;
  storedChunks: ProcessingChunkRecord[];
  totalAudioSec: number;
  processingProfile: ProcessingProfile;
  geminiApiKey: string;
  participantNames: string[];
  parsedSegmentsByChunk: Map<number, TranscriptionSegment[]>;
  segmentJsonByChunk: Map<number, string>;
  commitLiveState: CommitLiveState;
  addLiveLog: AddLiveLog;
  updatePipelineProgress: UpdatePipelineProgress;
  patchStoredChunk: PatchStoredChunk;
};

export const createFactExtractionQueue = ({
  meetingId,
  storedChunks,
  totalAudioSec,
  processingProfile,
  geminiApiKey,
  participantNames,
  parsedSegmentsByChunk,
  segmentJsonByChunk,
  commitLiveState,
  addLiveLog,
  updatePipelineProgress,
  patchStoredChunk,
}: FactExtractionQueueOptions) => {
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

  const reportProgress = () => {
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

  const markBatchAsRunning = async (batch: FactBatchItem[]) => {
    const runningPersists = batch.map((item) =>
      updateProcessingChunkFacts(meetingId, item.chunk.index, "running").catch(() => {}),
    );
    for (const item of batch) {
      patchStoredChunk(item.chunk.index, { factsStatus: "running" });
    }
    return runningPersists;
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
          const runningPersists = await markBatchAsRunning(batch);
          try {
            const insightsList = await extractFactBatch(batch, geminiApiKey, participantNames);
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
            reportProgress();
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
            geminiApiKey,
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
          reportProgress();
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

  const scheduleChunk = (
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
      reportProgress();
      return;
    }
    parsedSegmentsByChunk.set(chunk.index, segmentsForChunk);
    const segmentsJson = segmentJsonByChunk.get(chunk.index) ?? JSON.stringify(segmentsForChunk);
    segmentJsonByChunk.set(chunk.index, segmentsJson);
    factQueue.push({ chunk, segments: segmentsForChunk, segmentsJson });
    drainFactQueue();
  };

  const closeInput = () => {
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

  const setPhaseVisible = (visible: boolean) => {
    factsPhaseVisible = visible;
  };

  return {
    closeInput,
    reportProgress,
    scheduleChunk,
    setPhaseVisible,
  };
};

export type FactExtractionQueue = ReturnType<typeof createFactExtractionQueue>;
