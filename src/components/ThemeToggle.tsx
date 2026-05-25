import { useEffect, useMemo, useState } from "react";
import {
  applyThemePreference,
  nextThemePreference,
  normalizeThemePreference,
  readThemePreference,
  writeThemePreference,
  type ThemePreference,
} from "../lib/theme";

const labelByPreference: Record<ThemePreference, string> = {
  light: "Claro",
  dark: "Escuro",
  system: "Sistema",
};

export default function ThemeToggle() {
  const storage = typeof window === "undefined" ? undefined : window.localStorage;
  const mediaQuery = useMemo(
    () =>
      typeof window === "undefined"
        ? null
        : window.matchMedia?.("(prefers-color-scheme: dark)") ?? null,
    [],
  );
  const [preference, setPreference] = useState<ThemePreference>(() => readThemePreference(storage));

  useEffect(() => {
    const apply = () =>
      applyThemePreference(
        typeof document === "undefined" ? undefined : document.documentElement,
        preference,
        Boolean(mediaQuery?.matches),
      );

    apply();
    writeThemePreference(storage, preference);
    mediaQuery?.addEventListener?.("change", apply);
    return () => mediaQuery?.removeEventListener?.("change", apply);
  }, [mediaQuery, preference, storage]);

  const cycleTheme = () => setPreference((current) => nextThemePreference(normalizeThemePreference(current)));

  return (
    <button
      type="button"
      onClick={cycleTheme}
      className="rounded-lg border border-gray-200 bg-white px-3 py-2 text-xs font-semibold text-gray-600 shadow-sm transition hover:bg-gray-50 hover:text-gray-950"
      title="Alternar tema"
      aria-label={`Tema: ${labelByPreference[preference]}`}
    >
      {labelByPreference[preference]}
    </button>
  );
}
