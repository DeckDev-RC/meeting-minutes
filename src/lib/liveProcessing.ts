import type { DiarizedSegment, MeetingChunkInsights, TranscriptionSegment } from "./types";

export type LiveTab = "transcript" | "insights" | "minutes" | "logs";
export type LiveLogLevel = "info" | "success" | "warning" | "error";

export interface LiveTranscriptItem {
  id: string;
  chunkIndex: number;
  start: number;
  end: number;
  timeLabel: string;
  endTimeLabel: string;
  text: string;
  speaker: string;
  segmentCount: number;
}

export interface LiveInsightItem {
  id: string;
  chunkIndex: number;
  startSec: number;
  endSec: number;
  timeLabel: string;
  summary: string;
  topics: string[];
  decisionCount: number;
  actionCount: number;
  questionCount: number;
  riskCount: number;
  decisions: string[];
  actions: string[];
  risks: string[];
}

export interface LiveLogItem {
  id: string;
  level: LiveLogLevel;
  message: string;
  timeLabel: string;
}

export interface LiveProcessingState {
  transcript: LiveTranscriptItem[];
  insights: LiveInsightItem[];
  logs: LiveLogItem[];
  minutesDraft: string;
  finalMinutesText: string;
}

const DEFAULT_MAX_TRANSCRIPT_ITEMS = 160;
const DEFAULT_MAX_INSIGHTS = 80;
const DEFAULT_MAX_LOGS = 120;

export const createLiveProcessingState = (): LiveProcessingState => ({
  transcript: [],
  insights: [],
  logs: [],
  minutesDraft: "",
  finalMinutesText: "",
});

const finiteOrZero = (value: number) =>
  Number.isFinite(value) && value > 0 ? value : 0;

export const formatLiveTimestamp = (seconds: number) => {
  const totalSeconds = Math.floor(finiteOrZero(seconds));
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const secs = totalSeconds % 60;
  const mm = String(minutes).padStart(2, "0");
  const ss = String(secs).padStart(2, "0");

  if (hours > 0) {
    return `${String(hours).padStart(2, "0")}:${mm}:${ss}`;
  }

  return `${mm}:${ss}`;
};

const capTail = <T>(items: T[], maxItems: number) => {
  if (!Number.isFinite(maxItems) || maxItems <= 0) return items;
  return items.length > maxItems ? items.slice(items.length - maxItems) : items;
};

const mergeSortedByStart = <T extends { start: number }>(left: T[], right: T[]) => {
  const merged: T[] = [];
  let leftIndex = 0;
  let rightIndex = 0;

  while (leftIndex < left.length || rightIndex < right.length) {
    const leftItem = left[leftIndex];
    const rightItem = right[rightIndex];
    if (rightItem === undefined || (leftItem !== undefined && leftItem.start <= rightItem.start)) {
      merged.push(leftItem);
      leftIndex += 1;
    } else {
      merged.push(rightItem);
      rightIndex += 1;
    }
  }

  return merged;
};

const upsertSortedInsight = (
  insights: LiveInsightItem[],
  nextInsight: LiveInsightItem,
) => {
  const merged: LiveInsightItem[] = [];
  let inserted = false;

  for (const insight of insights) {
    if (insight.chunkIndex === nextInsight.chunkIndex) {
      continue;
    }
    if (!inserted && nextInsight.startSec <= insight.startSec) {
      merged.push(nextInsight);
      inserted = true;
    }
    merged.push(insight);
  }

  if (!inserted) {
    merged.push(nextInsight);
  }

  return merged;
};

const cleanText = (value: string) => value.replace(/\s+/g, " ").trim();

const shouldMergeTranscriptSegment = (
  current: LiveTranscriptItem,
  segment: TranscriptionSegment,
) => {
  const gapSec = segment.start - current.end;
  const mergedTextLength = current.text.length + cleanText(segment.text).length + 1;
  const mergedDuration = segment.end - current.start;

  return gapSec >= -0.25 && gapSec <= 3.5 && mergedTextLength <= 220 && mergedDuration <= 24;
};

const buildReadableTranscriptBlocks = (
  chunkIndex: number,
  segments: TranscriptionSegment[],
): LiveTranscriptItem[] => {
  const sortedSegments = [...segments]
    .map((segment) => ({ ...segment, text: cleanText(segment.text) }))
    .filter((segment) => segment.text.length > 0)
    .sort((a, b) => a.start - b.start);
  const blocks: LiveTranscriptItem[] = [];

  for (const segment of sortedSegments) {
    const current = blocks.length > 0 ? blocks[blocks.length - 1] : undefined;
    if (current && shouldMergeTranscriptSegment(current, segment)) {
      const end = Math.max(current.end, segment.end);
      blocks[blocks.length - 1] = {
        ...current,
        id: `${current.id}:${segment.id}`,
        end,
        endTimeLabel: formatLiveTimestamp(end),
        text: `${current.text} ${segment.text}`,
        segmentCount: current.segmentCount + 1,
      };
      continue;
    }

    blocks.push({
      id: `${chunkIndex}:${segment.id}:${segment.start}:${segment.end}`,
      chunkIndex,
      start: segment.start,
      end: segment.end,
      timeLabel: formatLiveTimestamp(segment.start),
      endTimeLabel: formatLiveTimestamp(segment.end),
      text: segment.text,
      speaker: "Falante em analise",
      segmentCount: 1,
    });
  }

  return blocks;
};

export const appendLiveTranscript = (
  state: LiveProcessingState,
  chunkIndex: number,
  segments: TranscriptionSegment[],
  maxItems = DEFAULT_MAX_TRANSCRIPT_ITEMS,
): LiveProcessingState => {
  const additions = buildReadableTranscriptBlocks(chunkIndex, segments);

  const transcript = capTail(mergeSortedByStart(state.transcript, additions), maxItems);

  return { ...state, transcript };
};

const overlapSeconds = (
  firstStart: number,
  firstEnd: number,
  secondStart: number,
  secondEnd: number,
) => Math.max(0, Math.min(firstEnd, secondEnd) - Math.max(firstStart, secondStart));

export const applyLiveTranscriptSpeakers = (
  state: LiveProcessingState,
  diarizedSegments: DiarizedSegment[],
): LiveProcessingState => {
  if (diarizedSegments.length === 0 || state.transcript.length === 0) return state;
  const sortedSegments = [...diarizedSegments].sort((a, b) => a.start - b.start);
  let cursor = 0;

  const transcript = state.transcript.map((item) => {
    let bestSpeaker = "";
    let bestOverlap = 0;

    while (cursor < sortedSegments.length && sortedSegments[cursor].end < item.start) {
      cursor += 1;
    }

    for (let index = cursor; index < sortedSegments.length; index += 1) {
      const segment = sortedSegments[index];
      if (segment.start > item.end) break;
      const overlap = overlapSeconds(item.start, item.end, segment.start, segment.end);
      if (overlap > bestOverlap) {
        bestOverlap = overlap;
        bestSpeaker = segment.speaker;
      }
    }

    return bestSpeaker ? { ...item, speaker: bestSpeaker } : item;
  });

  return { ...state, transcript };
};

export const appendLiveInsights = (
  state: LiveProcessingState,
  insights: MeetingChunkInsights,
  maxItems = DEFAULT_MAX_INSIGHTS,
  participantNames: string[] = [],
): LiveProcessingState => {
  const nextInsight: LiveInsightItem = {
    id: `chunk:${insights.chunkIndex}`,
    chunkIndex: insights.chunkIndex,
    startSec: insights.startSec,
    endSec: insights.endSec,
    timeLabel: formatLiveTimestamp(insights.startSec),
    summary: cleanText(insights.summary),
    topics: insights.topics.map(cleanText).filter(Boolean).slice(0, 6),
    decisionCount: insights.decisions.length,
    actionCount: insights.actions.length,
    questionCount: insights.questions.length,
    riskCount: insights.risks.length,
    decisions: insights.decisions.map((item) => cleanText(item.title)).filter(Boolean),
    actions: insights.actions.map((item) => cleanText(item.task)).filter(Boolean),
    risks: insights.risks.map(cleanText).filter(Boolean),
  };

  const insightsList = capTail(upsertSortedInsight(state.insights, nextInsight), maxItems);

  return {
    ...state,
    insights: insightsList,
    minutesDraft: buildLiveMinutesDraft(insightsList, participantNames),
  };
};

export const appendLiveLog = (
  state: LiveProcessingState,
  level: LiveLogLevel,
  message: string,
  elapsedSec: number,
  maxItems = DEFAULT_MAX_LOGS,
): LiveProcessingState => {
  const cleanMessage = cleanText(message);
  if (!cleanMessage) return state;

  const logs = capTail(
    [
      ...state.logs,
      {
        id: `${Date.now()}:${state.logs.length}:${cleanMessage}`,
        level,
        message: cleanMessage,
        timeLabel: formatLiveTimestamp(elapsedSec),
      },
    ],
    maxItems,
  );

  return { ...state, logs };
};

export const setFinalMinutesText = (
  state: LiveProcessingState,
  finalMinutesText: string,
): LiveProcessingState => ({
  ...state,
  finalMinutesText: cleanText(finalMinutesText),
});

export const buildLiveMinutesDraft = (
  insights: LiveInsightItem[],
  participantNames: string[] = [],
) => {
  const summaries = insights.map((item) => item.summary).filter(Boolean).slice(-6);
  const topics = Array.from(new Set(insights.flatMap((item) => item.topics))).slice(0, 10);
  const decisions = insights.flatMap((item) => item.decisions).slice(-8);
  const actions = insights.flatMap((item) => item.actions).slice(-10);
  const risks = insights.flatMap((item) => item.risks).slice(-6);
  const output: string[] = [];

  output.push("Rascunho vivo da ata");
  if (participantNames.length > 0) {
    output.push(`Participantes: ${participantNames.join(", ")}`);
  }
  if (topics.length > 0) {
    output.push(`Pauta em formação: ${topics.join("; ")}`);
  }
  if (summaries.length > 0) {
    output.push("");
    output.push("Resumo parcial:");
    output.push(...summaries.map((item) => `- ${item}`));
  }
  if (decisions.length > 0) {
    output.push("");
    output.push("Decisões detectadas:");
    output.push(...decisions.map((item) => `- ${item}`));
  }
  if (actions.length > 0) {
    output.push("");
    output.push("Ações detectadas:");
    output.push(...actions.map((item) => `- ${item}`));
  }
  if (risks.length > 0) {
    output.push("");
    output.push("Riscos e bloqueios:");
    output.push(...risks.map((item) => `- ${item}`));
  }

  return output.join("\n");
};
