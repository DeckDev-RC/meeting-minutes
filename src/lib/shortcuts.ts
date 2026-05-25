import { useEffect } from "react";

export type ShortcutDefinition = {
  key: string;
  mod?: boolean;
  alt?: boolean;
  shift?: boolean;
};

export type ShortcutHandler = ShortcutDefinition & {
  handler: () => void;
};

type MinimalKeyboardEvent = {
  key: string;
  ctrlKey?: boolean;
  metaKey?: boolean;
  altKey?: boolean;
  shiftKey?: boolean;
};

type MinimalTarget = {
  tagName?: string;
  isContentEditable?: boolean;
};

export function targetAllowsGlobalShortcut(target: MinimalTarget | null | undefined) {
  if (!target) return true;
  if (target.isContentEditable) return false;
  const tagName = target.tagName?.toUpperCase();
  return !["INPUT", "TEXTAREA", "SELECT"].includes(tagName ?? "");
}

export function shortcutMatches(event: MinimalKeyboardEvent, shortcut: ShortcutDefinition) {
  const keyMatches = event.key.toLowerCase() === shortcut.key.toLowerCase();
  if (!keyMatches) return false;
  if (Boolean(shortcut.mod) !== Boolean(event.ctrlKey || event.metaKey)) return false;
  if (Boolean(shortcut.alt) !== Boolean(event.altKey)) return false;
  if (Boolean(shortcut.shift) !== Boolean(event.shiftKey)) return false;
  return true;
}

export function useGlobalShortcuts(shortcuts: ShortcutHandler[]) {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (!targetAllowsGlobalShortcut(event.target as MinimalTarget | null)) return;
      const match = shortcuts.find((shortcut) => shortcutMatches(event, shortcut));
      if (!match) return;
      event.preventDefault();
      match.handler();
    };

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [shortcuts]);
}
