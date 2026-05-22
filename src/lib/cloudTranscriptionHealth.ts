export interface StorageLike {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

export interface CloudflareQuotaState {
  isExhaustedToday: boolean;
  dateKey?: string;
  exhaustedAt?: string;
  reason?: string;
}

const CLOUDFLARE_QUOTA_EXHAUSTED_KEY = "meeting-minutes.cloudflareQuotaExhausted";

function localDateKey(now = new Date()) {
  const year = now.getFullYear();
  const month = String(now.getMonth() + 1).padStart(2, "0");
  const day = String(now.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

function parseStoredQuotaState(raw: string | null): Omit<CloudflareQuotaState, "isExhaustedToday"> {
  if (!raw) return {};
  try {
    const parsed = JSON.parse(raw) as Partial<CloudflareQuotaState>;
    return {
      dateKey: typeof parsed.dateKey === "string" ? parsed.dateKey : undefined,
      exhaustedAt: typeof parsed.exhaustedAt === "string" ? parsed.exhaustedAt : undefined,
      reason: typeof parsed.reason === "string" ? parsed.reason : undefined,
    };
  } catch {
    return {};
  }
}

export function getCloudflareQuotaState(
  storage: StorageLike | undefined,
  now = new Date(),
): CloudflareQuotaState {
  if (!storage) return { isExhaustedToday: false };
  const stored = parseStoredQuotaState(storage.getItem(CLOUDFLARE_QUOTA_EXHAUSTED_KEY));
  const today = localDateKey(now);
  return {
    ...stored,
    isExhaustedToday: stored.dateKey === today,
  };
}

export function markCloudflareQuotaExhausted(
  storage: StorageLike | undefined,
  now = new Date(),
  reason = "Cloudflare quota exhausted",
) {
  if (!storage) return;
  storage.setItem(
    CLOUDFLARE_QUOTA_EXHAUSTED_KEY,
    JSON.stringify({
      dateKey: localDateKey(now),
      exhaustedAt: now.toISOString(),
      reason: reason.slice(0, 320),
    }),
  );
}

export function clearCloudflareQuotaExhausted(storage: StorageLike | undefined) {
  storage?.removeItem(CLOUDFLARE_QUOTA_EXHAUSTED_KEY);
}
