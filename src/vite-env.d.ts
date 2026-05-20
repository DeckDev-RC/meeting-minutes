/// <reference types="vite/client" />

interface Window {
  __MEETING_MINUTES_E2E__?: {
    invoke?: <T = unknown>(command: string, args?: Record<string, unknown>) => Promise<T>;
    listen?: <T = unknown>(
      event: string,
      handler: (event: { payload: T }) => void,
    ) => Promise<() => void>;
  };
}
