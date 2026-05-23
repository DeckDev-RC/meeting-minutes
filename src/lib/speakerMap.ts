import type { DiarizedSegment } from './types';

export type SpeakerMap = Record<string, string>;

function normalizeLabel(value: string) {
  return value.trim().replace(/\s+/g, ' ');
}

function speakerSortKey(label: string) {
  const match = label.match(/^(?:Falante|Speaker)\s+(\d+)$/i);
  return match ? Number(match[1]) : Number.POSITIVE_INFINITY;
}

function escapeRegExp(value: string) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

export function extractSpeakerLabels(
  speakers: string[] = [],
  diarizedSegments: Pick<DiarizedSegment, 'speaker'>[] = [],
) {
  const labels = new Set<string>();
  for (const speaker of speakers) {
    const label = normalizeLabel(speaker);
    if (label) labels.add(label);
  }
  for (const segment of diarizedSegments) {
    const label = normalizeLabel(segment.speaker);
    if (label) labels.add(label);
  }

  return Array.from(labels).sort((a, b) => {
    const aKey = speakerSortKey(a);
    const bKey = speakerSortKey(b);
    if (aKey !== bKey) return aKey - bKey;
    return a.localeCompare(b, 'pt-BR');
  });
}

export function normalizeSpeakerMap(labels: string[], draft: SpeakerMap): SpeakerMap {
  const allowed = new Set(labels.map(normalizeLabel));
  const normalized: SpeakerMap = {};
  for (const [rawSpeaker, rawName] of Object.entries(draft)) {
    const speaker = normalizeLabel(rawSpeaker);
    const name = normalizeLabel(rawName);
    if (!allowed.has(speaker) || !name || name === speaker) continue;
    normalized[speaker] = name.slice(0, 120);
  }
  return normalized;
}

export function parseSpeakerMapJson(raw: string | null | undefined): SpeakerMap {
  if (!raw) return {};
  try {
    const parsed = JSON.parse(raw) as unknown;
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return {};
    return Object.fromEntries(
      Object.entries(parsed as Record<string, unknown>)
        .filter((entry): entry is [string, string] => typeof entry[1] === 'string')
        .map(([speaker, name]) => [normalizeLabel(speaker), normalizeLabel(name)])
        .filter(([speaker, name]) => Boolean(speaker && name)),
    );
  } catch {
    return {};
  }
}

export function applySpeakerMapToText(text: string, speakerMap: SpeakerMap) {
  const entries = Object.entries(speakerMap)
    .map(([speaker, name]) => [normalizeLabel(speaker), normalizeLabel(name)] as const)
    .filter(([speaker, name]) => speaker && name && speaker !== name)
    .sort((a, b) => b[0].length - a[0].length);

  return entries.reduce((current, [speaker, name]) => {
    const pattern = new RegExp(`\\b${escapeRegExp(speaker)}\\b`, 'g');
    return current.replace(pattern, name);
  }, text);
}
