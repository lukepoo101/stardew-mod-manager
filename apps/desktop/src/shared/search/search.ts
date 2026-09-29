export type SearchKind = "page" | "mod" | "profile" | "setting";

export interface SearchEntry {
  id: string;
  kind: SearchKind;
  title: string;
  subtitle?: string;
  /** Extra words that should find the entry without being shown. */
  keywords?: string[];
  /** Route to open, relative to the app router. */
  to: string;
}

function normalise(text: string): string {
  return text.toLowerCase().normalize("NFKD").replace(/\p{M}/gu, "");
}

function scoreEntry(entry: SearchEntry, query: string): number {
  const title = normalise(entry.title);
  if (title === query) return 100;
  if (title.startsWith(query)) return 80;
  if (title.split(/[\s._-]+/).some((word) => word.startsWith(query))) return 60;
  if (title.includes(query)) return 40;
  const rest = normalise(
    [entry.subtitle ?? "", ...(entry.keywords ?? [])].join(" "),
  );
  if (rest.includes(query)) return 20;
  return 0;
}

const KIND_ORDER: Record<SearchKind, number> = {
  page: 0,
  profile: 1,
  mod: 2,
  setting: 3,
};

/**
 * Ranks entries for a query. Every word must match somewhere, results are
 * ordered by best match then kind then title so the order is stable, and an
 * empty query lists pages and settings rather than nothing.
 */
export function rankEntries(
  entries: readonly SearchEntry[],
  query: string,
  limit = 30,
): SearchEntry[] {
  const words = normalise(query).split(/\s+/).filter(Boolean);
  if (words.length === 0) {
    return entries
      .filter((entry) => entry.kind === "page" || entry.kind === "setting")
      .slice(0, limit);
  }
  const scored: { entry: SearchEntry; score: number }[] = [];
  for (const entry of entries) {
    let total = 0;
    let matchesAll = true;
    for (const word of words) {
      const score = scoreEntry(entry, word);
      if (score === 0) {
        matchesAll = false;
        break;
      }
      total += score;
    }
    if (matchesAll) scored.push({ entry, score: total });
  }
  scored.sort(
    (a, b) =>
      b.score - a.score ||
      KIND_ORDER[a.entry.kind] - KIND_ORDER[b.entry.kind] ||
      a.entry.title.localeCompare(b.entry.title),
  );
  return scored.slice(0, limit).map((item) => item.entry);
}

export const PAGE_ENTRIES: SearchEntry[] = [
  {
    id: "page-overview",
    kind: "page",
    title: "Overview",
    to: "/app/overview",
    keywords: ["home", "play", "launch"],
  },
  {
    id: "page-mods",
    kind: "page",
    title: "Mods",
    to: "/app/mods",
    keywords: ["installed", "install", "enable", "disable"],
  },
  {
    id: "page-profiles",
    kind: "page",
    title: "Profiles",
    to: "/app/profiles",
    keywords: ["recipe", "bundle", "export", "import"],
  },
  {
    id: "page-diagnostics",
    kind: "page",
    title: "Diagnostics",
    to: "/app/diagnostics",
    keywords: ["log", "errors", "support", "troubleshoot"],
  },
  {
    id: "page-activity",
    kind: "page",
    title: "Activity",
    to: "/app/activity",
    keywords: ["history", "operations"],
  },
  {
    id: "page-settings",
    kind: "page",
    title: "Settings",
    to: "/app/settings",
    keywords: ["preferences"],
  },
];

export const SETTING_ENTRIES: SearchEntry[] = [
  {
    id: "setting-theme",
    kind: "setting",
    title: "Appearance and theme",
    subtitle: "Settings",
    to: "/app/settings",
    keywords: ["dark", "light", "system", "colour", "color"],
  },
  {
    id: "setting-size",
    kind: "setting",
    title: "Interface size",
    subtitle: "Settings",
    to: "/app/settings",
    keywords: ["scale", "zoom", "text size", "high dpi"],
  },
  {
    id: "setting-games",
    kind: "setting",
    title: "Game installations",
    subtitle: "Settings",
    to: "/app/settings",
    keywords: ["steam", "gog", "path", "folder"],
  },
  {
    id: "setting-trust",
    kind: "setting",
    title: "Security and trust",
    subtitle: "Settings",
    to: "/app/settings",
    keywords: ["safe", "permissions", "sandbox"],
  },
];
