export type ExpiredLiveProcessingSnapshotInput = {
  completedAtByMeetingId: Map<string, number>;
  activeMeetingIds: Set<string>;
  visibleMeetingId: string | null;
  nowMs: number;
  ttlMs: number;
};

export type OverflowLiveProcessingSnapshotInput = {
  snapshotIds: string[];
  activeMeetingIds: Set<string>;
  visibleMeetingId: string | null;
  touchedAtByMeetingId: Map<string, number>;
  maxEntries: number;
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

export function collectOverflowLiveProcessingSnapshotIds({
  snapshotIds,
  activeMeetingIds,
  visibleMeetingId,
  touchedAtByMeetingId,
  maxEntries,
}: OverflowLiveProcessingSnapshotInput): string[] {
  if (!Number.isFinite(maxEntries) || maxEntries <= 0 || snapshotIds.length <= maxEntries) {
    return [];
  }

  const removable = snapshotIds
    .filter((meetingId) => !activeMeetingIds.has(meetingId) && visibleMeetingId !== meetingId)
    .sort(
      (left, right) =>
        (touchedAtByMeetingId.get(left) ?? 0) - (touchedAtByMeetingId.get(right) ?? 0),
    );

  const protectedCount = snapshotIds.length - removable.length;
  const removeCount = Math.max(0, snapshotIds.length - Math.max(maxEntries, protectedCount));

  return removable.slice(0, removeCount);
}
