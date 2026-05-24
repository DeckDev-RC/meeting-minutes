import {
  buildLiveMinutesDraft,
  type LiveProcessingState,
} from "../../lib/liveProcessing";
import {
  collectExpiredLiveProcessingSnapshotIds,
  collectOverflowLiveProcessingSnapshotIds,
} from "../../lib/liveProcessingCache";

export const activeProcessingRuns = new Map<string, Promise<void>>();
export const pendingProcessingStartTimers = new Map<
  string,
  ReturnType<typeof window.setTimeout>
>();
export const liveProcessingSnapshots = new Map<string, LiveProcessingState>();
export const liveProcessingPublishers = new Map<
  string,
  (state: LiveProcessingState) => void
>();
export const liveProcessingPublishTimers = new Map<
  string,
  ReturnType<typeof window.setTimeout>
>();
export const liveProcessingLastPublishedAt = new Map<string, number>();
export const liveProcessingTouchedAt = new Map<string, number>();
export const completedProcessingSnapshots = new Map<string, number>();
export const minutesStreamRawSnapshots = new Map<string, string>();
export const liveProcessingParticipantNames = new Map<string, string[]>();

let visibleProcessingMeetingId: string | null = null;

const LIVE_STATE_PUBLISH_INTERVAL_MS = 160;
const COMPLETED_SNAPSHOT_TTL_MS = 10 * 60 * 1000;
export const COMPLETED_SNAPSHOT_CLEANUP_INTERVAL_MS = 5 * 60 * 1000;
const MAX_LIVE_PROCESSING_SNAPSHOTS = 5;

export const setVisibleProcessingMeetingId = (meetingId: string | null) => {
  visibleProcessingMeetingId = meetingId;
};

export const isVisibleProcessingMeeting = (meetingId: string) =>
  visibleProcessingMeetingId === meetingId;

export const flushLiveStateSnapshot = (meetingId: string) => {
  const timer = liveProcessingPublishTimers.get(meetingId);
  if (timer) {
    window.clearTimeout(timer);
    liveProcessingPublishTimers.delete(meetingId);
  }
  const publish = liveProcessingPublishers.get(meetingId);
  const snapshot = liveProcessingSnapshots.get(meetingId);
  if (visibleProcessingMeetingId === meetingId && publish && snapshot) {
    const participantNames = liveProcessingParticipantNames.get(meetingId) ?? [];
    const publishSnapshot =
      snapshot.finalMinutesText || snapshot.insights.length === 0
        ? snapshot
        : {
            ...snapshot,
            minutesDraft: buildLiveMinutesDraft(snapshot.insights, participantNames),
          };
    if (publishSnapshot !== snapshot) {
      liveProcessingSnapshots.set(meetingId, publishSnapshot);
    }
    liveProcessingLastPublishedAt.set(meetingId, Date.now());
    publish(publishSnapshot);
  }
};

export const scheduleLiveStatePublish = (meetingId: string) => {
  if (visibleProcessingMeetingId !== meetingId || !liveProcessingPublishers.has(meetingId)) {
    return;
  }
  const now = Date.now();
  const lastPublishedAt = liveProcessingLastPublishedAt.get(meetingId) ?? 0;
  const waitMs = LIVE_STATE_PUBLISH_INTERVAL_MS - (now - lastPublishedAt);

  if (waitMs <= 0) {
    flushLiveStateSnapshot(meetingId);
    return;
  }

  if (!liveProcessingPublishTimers.has(meetingId)) {
    const timer = window.setTimeout(() => {
      liveProcessingPublishTimers.delete(meetingId);
      flushLiveStateSnapshot(meetingId);
    }, waitMs);
    liveProcessingPublishTimers.set(meetingId, timer);
  }
};

export const cleanupCompletedLiveProcessingSnapshots = () => {
  const activeMeetingIds = new Set(activeProcessingRuns.keys());
  const expiredIds = collectExpiredLiveProcessingSnapshotIds({
    completedAtByMeetingId: completedProcessingSnapshots,
    activeMeetingIds,
    visibleMeetingId: visibleProcessingMeetingId,
    nowMs: Date.now(),
    ttlMs: COMPLETED_SNAPSHOT_TTL_MS,
  });
  const overflowIds = collectOverflowLiveProcessingSnapshotIds({
    snapshotIds: Array.from(liveProcessingSnapshots.keys()),
    activeMeetingIds,
    visibleMeetingId: visibleProcessingMeetingId,
    touchedAtByMeetingId: liveProcessingTouchedAt,
    maxEntries: MAX_LIVE_PROCESSING_SNAPSHOTS,
  });

  for (const meetingId of new Set([...expiredIds, ...overflowIds])) {
    const timer = liveProcessingPublishTimers.get(meetingId);
    if (timer) {
      window.clearTimeout(timer);
      liveProcessingPublishTimers.delete(meetingId);
    }
    liveProcessingSnapshots.delete(meetingId);
    minutesStreamRawSnapshots.delete(meetingId);
    liveProcessingParticipantNames.delete(meetingId);
    completedProcessingSnapshots.delete(meetingId);
    liveProcessingLastPublishedAt.delete(meetingId);
    liveProcessingTouchedAt.delete(meetingId);
  }
};

export const markProcessingSnapshotCompleted = (meetingId: string) => {
  completedProcessingSnapshots.set(meetingId, Date.now());
  flushLiveStateSnapshot(meetingId);
  cleanupCompletedLiveProcessingSnapshots();
};
