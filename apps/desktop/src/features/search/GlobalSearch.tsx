import React, { useEffect, useMemo, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import * as RadixDialog from "@radix-ui/react-dialog";
import {
  useActiveProfileOverview,
  useProfileMods,
  useProfiles,
} from "@/shared/api/hooks";
import {
  PAGE_ENTRIES,
  rankEntries,
  SETTING_ENTRIES,
  type SearchEntry,
} from "@/shared/search/search";

const KIND_LABEL: Record<SearchEntry["kind"], string> = {
  page: "Page",
  profile: "Profile",
  mod: "Mod",
  setting: "Setting",
};

/**
 * Finds pages, settings, profiles and installed mods from one keyboard-driven
 * box. Opened with Ctrl+K (Cmd+K on macOS) or the Search button; arrows move,
 * Enter opens, Escape closes.
 */
export const GlobalSearch: React.FC<{ open: boolean; onClose: () => void }> = ({
  open,
  onClose,
}) => {
  const navigate = useNavigate();
  const { data: overview } = useActiveProfileOverview();
  const { data: mods, error: modsError } = useProfileMods(
    open ? overview?.profile.id : undefined,
  );
  const { data: profiles, error: profilesError } = useProfiles();
  // Sources that could not be loaded, so "no results" is never claimed for
  // things that were not searched.
  const unavailable = [
    modsError ? "installed mods" : null,
    profilesError ? "profiles" : null,
  ].filter((part): part is string => part !== null);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  const entries = useMemo<SearchEntry[]>(
    () => [
      ...PAGE_ENTRIES,
      ...SETTING_ENTRIES,
      ...(profiles ?? []).map((profile) => ({
        id: `profile-${profile.id}`,
        kind: "profile" as const,
        title: profile.name,
        subtitle: `${profile.mod_count} mod(s)`,
        to: "/app/profiles",
      })),
      ...(mods ?? []).map((mod) => ({
        id: `mod-${mod.profile_component_id}`,
        kind: "mod" as const,
        title: mod.name,
        subtitle: mod.unique_id,
        keywords: [mod.author],
        to: `/app/mods?q=${encodeURIComponent(mod.unique_id)}`,
      })),
    ],
    [mods, profiles],
  );
  const results = useMemo(() => rankEntries(entries, query), [entries, query]);

  useEffect(() => {
    setActive(0);
  }, [query]);
  useEffect(() => {
    if (open) setQuery("");
  }, [open]);

  const choose = (entry: SearchEntry | undefined) => {
    if (!entry) return;
    onClose();
    navigate(entry.to);
  };

  const onKeyDown = (event: React.KeyboardEvent) => {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setActive((index) => Math.min(index + 1, results.length - 1));
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      setActive((index) => Math.max(index - 1, 0));
    } else if (event.key === "Home") {
      event.preventDefault();
      setActive(0);
    } else if (event.key === "End") {
      event.preventDefault();
      setActive(Math.max(results.length - 1, 0));
    } else if (event.key === "Enter") {
      event.preventDefault();
      choose(results[active]);
    }
  };

  return (
    <RadixDialog.Root
      open={open}
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
    >
      <RadixDialog.Portal>
        <RadixDialog.Overlay className="fixed inset-0 z-50 bg-black/50" />
        <RadixDialog.Content
          className="fixed z-50 top-24 left-1/2 -translate-x-1/2 w-[min(36rem,calc(100vw-2rem))] bg-[var(--bg-surface)] border border-[var(--border)] rounded-2xl shadow-xl overflow-hidden focus:outline-none"
          onOpenAutoFocus={(event) => {
            event.preventDefault();
            inputRef.current?.focus();
          }}
        >
          <RadixDialog.Title className="sr-only">Search</RadixDialog.Title>
          <RadixDialog.Description className="sr-only">
            Search pages, settings, profiles and installed mods. Use the arrow
            keys to choose and Enter to open.
          </RadixDialog.Description>
          <input
            ref={inputRef}
            role="combobox"
            aria-expanded="true"
            aria-controls="global-search-results"
            aria-activedescendant={
              results[active] ? `search-${results[active].id}` : undefined
            }
            aria-label="Search"
            placeholder="Search pages, settings, profiles and mods"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={onKeyDown}
            className="w-full px-4 py-3 bg-transparent border-b border-[var(--border)] text-sm outline-none"
          />
          <div
            id="global-search-results"
            role="listbox"
            aria-label="Search results"
            className="max-h-80 overflow-y-auto p-2"
          >
            {results.map((entry, index) => (
              <div
                key={entry.id}
                id={`search-${entry.id}`}
                role="option"
                tabIndex={-1}
                aria-selected={index === active}
                onMouseEnter={() => setActive(index)}
                onClick={() => choose(entry)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") choose(entry);
                }}
                className={`px-3 py-2 rounded-lg cursor-pointer flex items-center justify-between gap-3 text-sm ${
                  index === active ? "bg-[var(--bg-elevated)]" : ""
                }`}
              >
                <span className="min-w-0">
                  <span className="font-medium">{entry.title}</span>
                  {entry.subtitle && (
                    <span className="block text-xs text-[var(--fg-muted)] font-mono truncate">
                      {entry.subtitle}
                    </span>
                  )}
                </span>
                <span className="text-[10px] uppercase tracking-wider text-[var(--fg-muted)] shrink-0">
                  {KIND_LABEL[entry.kind]}
                </span>
              </div>
            ))}
            {results.length === 0 && (
              <div
                role="status"
                className="px-3 py-6 text-center text-xs text-[var(--fg-muted)]"
              >
                Nothing matches "{query}". Try a mod name, a page, or a setting.
              </div>
            )}
            {unavailable.length > 0 && (
              <div
                role="alert"
                className="px-3 py-2 text-xs text-[var(--warning)]"
              >
                {unavailable.join(" and ")} could not be loaded, so they were
                not searched.
              </div>
            )}
          </div>
        </RadixDialog.Content>
      </RadixDialog.Portal>
    </RadixDialog.Root>
  );
};
