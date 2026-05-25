export type ThemePreference = "light" | "dark" | "system";
export type ThemeMode = "light" | "dark";

const STORAGE_KEY = "meeting-minutes-theme";

export function normalizeThemePreference(value: unknown): ThemePreference {
  return value === "light" || value === "dark" || value === "system" ? value : "system";
}

export function resolveThemeMode(preference: ThemePreference, systemPrefersDark: boolean): ThemeMode {
  if (preference === "system") return systemPrefersDark ? "dark" : "light";
  return preference;
}

export function nextThemePreference(preference: ThemePreference): ThemePreference {
  if (preference === "light") return "dark";
  if (preference === "dark") return "system";
  return "light";
}

export function readThemePreference(storage: Storage | undefined): ThemePreference {
  if (!storage) return "system";
  return normalizeThemePreference(storage.getItem(STORAGE_KEY));
}

export function writeThemePreference(storage: Storage | undefined, preference: ThemePreference) {
  storage?.setItem(STORAGE_KEY, preference);
}

export function applyThemePreference(
  documentElement: HTMLElement | undefined,
  preference: ThemePreference,
  systemPrefersDark: boolean,
) {
  const mode = resolveThemeMode(preference, systemPrefersDark);
  documentElement?.setAttribute("data-theme", mode);
  documentElement?.setAttribute("data-theme-preference", preference);
  return mode;
}
