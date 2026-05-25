import type {
  DiarizedSegment,
  MeetingAction,
  MeetingChunkInsights,
  MeetingDecision,
  MeetingTopic,
  TranscriptionSegment,
} from './types';

type EvidenceSegment = Pick<TranscriptionSegment | DiarizedSegment, 'start' | 'end' | 'text'>;
type EvidenceKind = 'topic' | 'decision' | 'action';

export interface EvidenceValidationItem {
  kind: EvidenceKind;
  chunkIndex: number;
  label: string;
  evidence: string;
  score: number;
  verified: boolean;
}

export interface EvidenceValidationSummary {
  total: number;
  verified: number;
  items: EvidenceValidationItem[];
}

const STOPWORDS = new Set([
  'a',
  'as',
  'ate',
  'com',
  'da',
  'das',
  'de',
  'do',
  'dos',
  'e',
  'em',
  'o',
  'os',
  'para',
  'pela',
  'pelo',
  'por',
  'que',
  'um',
  'uma',
]);

const MAX_LIST_ITEMS = 40;
const MAX_FACT_ITEMS = 80;
const MAX_TEXT_CHARS = 500;
const EVIDENCE_THRESHOLD = 0.58;

export function normalizeEvidenceText(text: string) {
  return text
    .normalize('NFD')
    .replace(/[\u0300-\u036f]/g, '')
    .toLocaleLowerCase('pt-BR')
    .replace(/[^a-z0-9]+/g, ' ')
    .replace(/\s+/g, ' ')
    .trim();
}

function tokenizeEvidence(text: string) {
  return normalizeEvidenceText(text)
    .split(' ')
    .map((token) => token.trim())
    .filter((token) => token.length > 1 && !STOPWORDS.has(token));
}

function uniqueStrings(values: string[], limit = MAX_LIST_ITEMS) {
  const seen = new Set<string>();
  const result: string[] = [];
  for (const value of values) {
    const trimmed = value.trim().replace(/\s+/g, ' ');
    const key = normalizeEvidenceText(trimmed);
    if (!trimmed || !key || seen.has(key)) continue;
    seen.add(key);
    result.push(trimmed.slice(0, MAX_TEXT_CHARS));
    if (result.length >= limit) break;
  }
  return result;
}

function stringValue(value: unknown) {
  return typeof value === 'string' ? value.trim().replace(/\s+/g, ' ') : '';
}

function finiteNumber(value: unknown, fallback: number) {
  return typeof value === 'number' && Number.isFinite(value) ? value : fallback;
}

function timestampWithinChunk(timestampSec: number, startSec: number, endSec: number) {
  return timestampSec >= startSec - 5 && timestampSec <= endSec + 5;
}

function sanitizeDecision(value: unknown, startSec: number, endSec: number): MeetingDecision | null {
  if (!value || typeof value !== 'object') return null;
  const raw = value as Partial<MeetingDecision>;
  const title = stringValue(raw.title);
  const evidence = stringValue(raw.evidence);
  const timestampSec = finiteNumber(raw.timestampSec, startSec);
  if (!title || !evidence || !timestampWithinChunk(timestampSec, startSec, endSec)) return null;
  return {
    title: title.slice(0, MAX_TEXT_CHARS),
    owner: stringValue(raw.owner).slice(0, 120),
    timestampSec,
    evidence: evidence.slice(0, MAX_TEXT_CHARS),
  };
}

function sanitizeAction(value: unknown, startSec: number, endSec: number): MeetingAction | null {
  if (!value || typeof value !== 'object') return null;
  const raw = value as Partial<MeetingAction>;
  const task = stringValue(raw.task);
  const evidence = stringValue(raw.evidence);
  const timestampSec = finiteNumber(raw.timestampSec, startSec);
  if (!task || !evidence || !timestampWithinChunk(timestampSec, startSec, endSec)) return null;
  return {
    task: task.slice(0, MAX_TEXT_CHARS),
    owner: stringValue(raw.owner).slice(0, 120),
    deadline: stringValue(raw.deadline).slice(0, 120),
    timestampSec,
    evidence: evidence.slice(0, MAX_TEXT_CHARS),
  };
}

function sanitizeTopicEvidence(value: unknown, startSec: number, endSec: number): MeetingTopic | null {
  if (!value || typeof value !== 'object') return null;
  const raw = value as Partial<MeetingTopic>;
  const title = stringValue(raw.title);
  const evidence = stringValue(raw.evidence);
  const timestampSec = finiteNumber(raw.timestampSec, startSec);
  if (!title || !evidence || !timestampWithinChunk(timestampSec, startSec, endSec)) return null;
  return {
    title: title.slice(0, MAX_TEXT_CHARS),
    timestampSec,
    evidence: evidence.slice(0, MAX_TEXT_CHARS),
  };
}

function uniqueTopicEvidence(values: MeetingTopic[], limit = MAX_LIST_ITEMS) {
  const seen = new Set<string>();
  const result: MeetingTopic[] = [];
  for (const value of values) {
    const key = normalizeEvidenceText(value.title);
    if (!key || seen.has(key)) continue;
    seen.add(key);
    result.push(value);
    if (result.length >= limit) break;
  }
  return result;
}

export function sanitizeMeetingChunkInsights(value: unknown): MeetingChunkInsights {
  const raw = value && typeof value === 'object' ? (value as Partial<MeetingChunkInsights>) : {};
  const chunkIndex = Math.max(0, Math.floor(finiteNumber(raw.chunkIndex, 0)));
  const startSec = Math.max(0, finiteNumber(raw.startSec, 0));
  const endSec = Math.max(startSec, finiteNumber(raw.endSec, startSec));
  const summary = stringValue(raw.summary).slice(0, MAX_TEXT_CHARS) || 'Trecho sem resumo estruturado.';
  const topicEvidence = Array.isArray(raw.topicEvidence)
    ? uniqueTopicEvidence(
        raw.topicEvidence
          .map((item) => sanitizeTopicEvidence(item, startSec, endSec))
          .filter((item): item is MeetingTopic => Boolean(item)),
      )
    : undefined;
  const topics = uniqueStrings([
    ...(Array.isArray(raw.topics) ? raw.topics.filter((item): item is string => typeof item === 'string') : []),
    ...(topicEvidence?.map((topic) => topic.title) ?? []),
  ]);

  const decisions = Array.isArray(raw.decisions)
    ? raw.decisions
        .map((item) => sanitizeDecision(item, startSec, endSec))
        .filter((item): item is MeetingDecision => Boolean(item))
        .slice(0, MAX_FACT_ITEMS)
    : [];
  const actions = Array.isArray(raw.actions)
    ? raw.actions
        .map((item) => sanitizeAction(item, startSec, endSec))
        .filter((item): item is MeetingAction => Boolean(item))
        .slice(0, MAX_FACT_ITEMS)
    : [];

  const sanitized: MeetingChunkInsights = {
    chunkIndex,
    startSec,
    endSec,
    summary,
    topics,
    decisions,
    actions,
    questions: Array.isArray(raw.questions)
      ? uniqueStrings(raw.questions.filter((item): item is string => typeof item === 'string'))
      : [],
    risks: Array.isArray(raw.risks)
      ? uniqueStrings(raw.risks.filter((item): item is string => typeof item === 'string'))
      : [],
  };
  if (topicEvidence) {
    sanitized.topicEvidence = topicEvidence;
  }
  return sanitized;
}

export function evidenceSimilarity(source: string, evidence: string) {
  const normalizedSource = normalizeEvidenceText(source);
  const normalizedEvidence = normalizeEvidenceText(evidence);
  if (!normalizedSource || !normalizedEvidence) return 0;
  if (normalizedSource.includes(normalizedEvidence)) return 1;

  const sourceTokens = tokenizeEvidence(source);
  const evidenceTokens = tokenizeEvidence(evidence);
  if (sourceTokens.length === 0 || evidenceTokens.length === 0) return 0;

  const sourceCounts = new Map<string, number>();
  for (const token of sourceTokens) {
    sourceCounts.set(token, (sourceCounts.get(token) ?? 0) + 1);
  }

  let matched = 0;
  for (const token of evidenceTokens) {
    const count = sourceCounts.get(token) ?? 0;
    if (count > 0) {
      matched += 1;
      sourceCounts.set(token, count - 1);
    }
  }

  if (matched === evidenceTokens.length) return 1;
  const coverage = matched / evidenceTokens.length;
  const dice = (2 * matched) / (sourceTokens.length + evidenceTokens.length);
  return Math.max(dice, coverage * 0.85);
}

function bestEvidenceScore(evidence: string, segments: EvidenceSegment[]) {
  const corpus = segments.map((segment) => segment.text).join(' ');
  let best = evidenceSimilarity(corpus, evidence);
  for (const segment of segments) {
    best = Math.max(best, evidenceSimilarity(segment.text, evidence));
  }
  return Math.min(1, Number(best.toFixed(3)));
}

export function validateMeetingInsightsEvidence(
  insights: MeetingChunkInsights,
  segments: EvidenceSegment[],
  threshold = EVIDENCE_THRESHOLD,
): EvidenceValidationSummary {
  const items: EvidenceValidationItem[] = [];
  const append = (kind: EvidenceKind, label: string, evidence: string) => {
    const score = bestEvidenceScore(evidence, segments);
    items.push({
      kind,
      chunkIndex: insights.chunkIndex,
      label,
      evidence,
      score,
      verified: score >= threshold,
    });
  };

  for (const topic of insights.topicEvidence ?? []) {
    append('topic', topic.title, topic.evidence);
  }
  for (const decision of insights.decisions) {
    append('decision', decision.title, decision.evidence);
  }
  for (const action of insights.actions) {
    append('action', action.task, action.evidence);
  }

  return {
    total: items.length,
    verified: items.filter((item) => item.verified).length,
    items,
  };
}

export function purgeUnverifiedMeetingInsightsEvidence(
  insights: MeetingChunkInsights,
  segments: EvidenceSegment[],
  threshold = EVIDENCE_THRESHOLD,
) {
  if (segments.length === 0) {
    return { insights, removed: [] as EvidenceValidationItem[] };
  }

  const removed: EvidenceValidationItem[] = [];
  const keepEvidence = (
    kind: EvidenceKind,
    label: string,
    evidence: string,
  ) => {
    const score = bestEvidenceScore(evidence, segments);
    const verified = score >= threshold;
    if (!verified) {
      removed.push({
        kind,
        chunkIndex: insights.chunkIndex,
        label,
        evidence,
        score,
        verified,
      });
    }
    return verified;
  };

  const hasTopicEvidence = Array.isArray(insights.topicEvidence);
  const topicEvidence = (insights.topicEvidence ?? []).filter((topic) =>
    keepEvidence('topic', topic.title, topic.evidence),
  );
  const verifiedTopicKeys = new Set(topicEvidence.map((topic) => normalizeEvidenceText(topic.title)));
  const knownTopicKeys = new Set((insights.topicEvidence ?? []).map((topic) => normalizeEvidenceText(topic.title)));
  const topics = hasTopicEvidence
    ? insights.topics.filter((topic) => {
        const key = normalizeEvidenceText(topic);
        if (verifiedTopicKeys.has(key)) return true;
        if (!knownTopicKeys.has(key)) {
          removed.push({
            kind: 'topic',
            chunkIndex: insights.chunkIndex,
            label: topic,
            evidence: '',
            score: 0,
            verified: false,
          });
        }
        return false;
      })
    : insights.topics;
  const decisions = insights.decisions.filter((decision) =>
    keepEvidence('decision', decision.title, decision.evidence),
  );
  const actions = insights.actions.filter((action) =>
    keepEvidence('action', action.task, action.evidence),
  );

  if (removed.length === 0) {
    return { insights, removed };
  }

  return {
    insights: {
      ...insights,
      topics,
      ...(hasTopicEvidence ? { topicEvidence } : {}),
      decisions,
      actions,
    },
    removed,
  };
}

export function summarizeEvidenceValidation(summaries: EvidenceValidationSummary[]) {
  const total = summaries.reduce((sum, item) => sum + item.total, 0);
  const verified = summaries.reduce((sum, item) => sum + item.verified, 0);
  return {
    total,
    verified,
    ratio: total === 0 ? 1 : verified / total,
  };
}

export function summarizeEvidencePurge(removed: EvidenceValidationItem[]) {
  const removedTopics = removed.filter((item) => item.kind === 'topic').length;
  const removedDecisions = removed.filter((item) => item.kind === 'decision').length;
  const removedActions = removed.filter((item) => item.kind === 'action').length;
  return {
    removedTopics,
    removedDecisions,
    removedActions,
    removedTotal: removedTopics + removedDecisions + removedActions,
  };
}
