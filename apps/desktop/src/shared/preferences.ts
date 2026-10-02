import { useCallback, useState } from "react";
import { MOD_SORTS, type ModSort } from "@/shared/mods/organise";

/**
 * Versioned, local-only UI preferences. Anything unreadable, from another
 * version or of the wrong shape is discarded and replaced by the default, so a
 * stale or hand-edited value can never break the interface. Preferences hold
 * presentation choices only; nothing that changes profile state lives here.
 */
const STORAGE_KEY = "smm-ui-preferences";
export const PREFERENCES_VERSION = 1;

export const UI_SCALES = [90, 100, 115, 130, 150] as const;
export type UiScale = (typeof UI_SCALES)[number];
export type ModFilter = "all" | "enabled" | "disabled";

export interface UiPreferences {
  uiScale: UiScale;
  modFilter: ModFilter;
  modSort: ModSort;
  /** Reverses the chosen sort. */
  modSortDescending: boolean;
  /** Explanatory text in empty states and similar guidance. Actions stay
   * visible either way. */
  showGuidance: boolean;
  /** Experts: hide informational findings by default. Errors and warnings
   * always show, and hidden findings are counted. */
  quietInfo: boolean;
  /** Shows advanced views (dependency map, installed file lists, operation
   * ids) and hides guidance text. Checks, severities, confirmations and
   * recovery are the same either way. */
  expertMode: boolean;
}

export const DEFAULT_PREFERENCES: UiPreferences = {
  uiScale: 100,
  modFilter: "all",
  modSort: "name",
  modSortDescending: false,
  showGuidance: true,
  quietInfo: false,
  expertMode: false,
};

function sanitize(raw: unknown): UiPreferences {
  if (typeof raw !== "object" || raw === null) return DEFAULT_PREFERENCES;
  const value = raw as Record<string, unknown>;
  return {
    uiScale: UI_SCALES.includes(value.uiScale as UiScale)
      ? (value.uiScale as UiScale)
      : DEFAULT_PREFERENCES.uiScale,
    modFilter: ["all", "enabled", "disabled"].includes(
      value.modFilter as string,
    )
      ? (value.modFilter as ModFilter)
      : DEFAULT_PREFERENCES.modFilter,
    modSort: MOD_SORTS.includes(value.modSort as ModSort)
      ? (value.modSort as ModSort)
      : DEFAULT_PREFERENCES.modSort,
    modSortDescending:
      typeof value.modSortDescending === "boolean"
        ? value.modSortDescending
        : DEFAULT_PREFERENCES.modSortDescending,
    showGuidance:
      typeof value.showGuidance === "boolean"
        ? value.showGuidance
        : DEFAULT_PREFERENCES.showGuidance,
    quietInfo:
      typeof value.quietInfo === "boolean"
        ? value.quietInfo
        : DEFAULT_PREFERENCES.quietInfo,
    expertMode:
      typeof value.expertMode === "boolean"
        ? value.expertMode
        : DEFAULT_PREFERENCES.expertMode,
  };
}

export function loadPreferences(): UiPreferences {
  try {
    const text = localStorage.getItem(STORAGE_KEY);
    if (!text) return DEFAULT_PREFERENCES;
    const parsed = JSON.parse(text) as { version?: number; values?: unknown };
    if (parsed.version !== PREFERENCES_VERSION) return DEFAULT_PREFERENCES;
    return sanitize(parsed.values);
  } catch {
    return DEFAULT_PREFERENCES;
  }
}

export function savePreferences(preferences: UiPreferences): void {
  try {
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({ version: PREFERENCES_VERSION, values: preferences }),
    );
  } catch {
    // Storage may be unavailable; the preference then lasts for this session.
  }
}

export function resetPreferences(): void {
  try {
    localStorage.removeItem(STORAGE_KEY);
  } catch {
    // Nothing to clear.
  }
}

/** Applies the scale to the document so every rem-based size follows it. */
export function applyUiScale(scale: UiScale): void {
  if (typeof document === "undefined") return;
  document.documentElement.style.fontSize = `${scale}%`;
}

export function usePreferences(): [
  UiPreferences,
  (patch: Partial<UiPreferences>) => void,
  () => void,
] {
  const [preferences, setPreferences] =
    useState<UiPreferences>(loadPreferences);
  const update = useCallback((patch: Partial<UiPreferences>) => {
    setPreferences((current) => {
      const next = sanitize({ ...current, ...patch });
      savePreferences(next);
      if (patch.uiScale !== undefined) applyUiScale(next.uiScale);
      return next;
    });
  }, []);
  const reset = useCallback(() => {
    resetPreferences();
    applyUiScale(DEFAULT_PREFERENCES.uiScale);
    setPreferences(DEFAULT_PREFERENCES);
  }, []);
  return [preferences, update, reset];
}
