import type { TranscriptionSegment } from "./types";

export interface TranscriptRiskInput {
  durationSec: number;
  segments: TranscriptionSegment[];
}

export interface TranscriptRiskScore {
  shouldEscalate: boolean;
  reasons: string[];
}

function normalizedTranscriptText(segments: TranscriptionSegment[]) {
  return segments.map((segment) => segment.text).join(" ").replace(/\s+/g, " ").trim();
}

export function scoreCloudflareTranscriptRisk({
  durationSec,
  segments,
}: TranscriptRiskInput): TranscriptRiskScore {
  const text = normalizedTranscriptText(segments);
  const reasons: string[] = [];
  const safeDuration = Number.isFinite(durationSec) && durationSec > 0 ? durationSec : 0;
  const charsPerSecond = safeDuration > 0 ? text.length / safeDuration : text.length;

  if (!text) {
    reasons.push("empty-transcript");
  }

  if (safeDuration >= 120 && charsPerSecond < 3) {
    reasons.push("low-text-density");
  }

  if (/\bEmanuela\b/i.test(text) && !/\bManuela\b/i.test(text)) {
    reasons.push("name-inconsistency-manuela");
  }

  if (/(.{18,})\s+\1\s+\1/i.test(text)) {
    reasons.push("repeated-text");
  }

  return {
    shouldEscalate: reasons.length > 0,
    reasons,
  };
}
