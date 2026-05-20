export type ExpiredLiveProcessingSnapshotInput = {
  completedAtByMeetingId: Map<string, number>;
  activeMeetingIds: Set<string>;
  visibleMeetingId: string | null;
  nowMs: number;
  ttlMs: number;
};

export function collectExpiredLiveProcessingSnapshotIds({
  completedAtByMeetingId,
  activeMeetingIds,
  visibleMeetingId,
  nowMs,
  ttlMs,
}: ExpiredLiveProcessingSnapshotInput): string[] {
  const expired: string[] = [];

  for (const [meetingId, completedAtMs] of completedAtByMeetingId) {
    if (activeMeetingIds.has(meetingId) || visibleMeetingId === meetingId) {
      continue;
    }
    if (nowMs - completedAtMs >= ttlMs) {
      expired.push(meetingId);
    }
  }

  return expired;
}
