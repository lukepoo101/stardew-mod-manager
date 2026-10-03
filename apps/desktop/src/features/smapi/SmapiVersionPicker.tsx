import React, { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { StatusBadge } from "@/components/ui/StatusBadge";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import type { SmapiReleaseDto } from "@/shared/api/generated";

const FIT: Record<
  string,
  { label: string; variant: "success" | "warning" | "danger" | "neutral" }
> = {
  compatible: { label: "Supports your game", variant: "success" },
  game_too_old: { label: "Needs a newer game", variant: "danger" },
  game_too_new: { label: "Too old for your game", variant: "danger" },
  unknown: { label: "Support unknown", variant: "neutral" },
};

const CHECKSUM: Record<string, string> = {
  published: "Checksum published by SMAPI",
  recorded: "Checksum recorded when first installed",
  none: "No published checksum",
};

function range(release: SmapiReleaseDto): string {
  if (!release.min_game) return "game versions not known";
  if (release.max_game && release.max_game === release.min_game)
    return `Stardew Valley ${release.min_game}`;
  if (release.max_game)
    return `Stardew Valley ${release.min_game} to ${release.max_game}`;
  return `Stardew Valley ${release.min_game} or newer`;
}

export function useSmapiCatalog(gameId: string | undefined) {
  return useQuery({
    queryKey: ["smapi-catalog", gameId],
    queryFn: () => api.getSmapiCatalog(gameId ?? ""),
    enabled: Boolean(gameId),
    staleTime: 60_000,
  });
}

/**
 * Every SMAPI release, newest first, with which game versions it supports
 * (as SMAPI declares), whether its download can be verified, and whether its
 * installer is kept here. Versions for this game are shown first; the rest
 * are a click away. Choosing changes nothing yet.
 */
export const SmapiVersionPicker: React.FC<{
  gameId: string;
  selected: string | undefined;
  onSelect: (version: string) => void;
}> = ({ gameId, selected, onSelect }) => {
  const client = useQueryClient();
  const { data: catalog, error } = useSmapiCatalog(gameId);
  const [showAll, setShowAll] = useState(false);
  const [showPre, setShowPre] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const refresh = async () => {
    setRefreshing(true);
    try {
      client.setQueryData(
        ["smapi-catalog", gameId],
        await api.getSmapiCatalog(gameId, true),
      );
    } finally {
      setRefreshing(false);
    }
  };
  if (error)
    return (
      <p role="alert" className="text-xs text-[var(--danger)]">
        {errorSummary(error, "The SMAPI releases could not be listed")}
      </p>
    );
  if (!catalog)
    return (
      <p role="status" className="text-xs text-[var(--fg-muted)]">
        Reading SMAPI's releases...
      </p>
    );
  const visible = catalog.releases.filter(
    (r) =>
      (showPre || !r.prerelease || r.is_installed) &&
      (showAll ||
        r.compatibility === "compatible" ||
        r.is_installed ||
        r.is_recommended ||
        (r.compatibility === "unknown" && !catalog.game_version)),
  );
  const hidden = catalog.releases.length - visible.length;
  return (
    <div className="space-y-2 text-xs">
      <p className="text-[var(--fg-muted)]">
        {catalog.game_version
          ? `Your game: Stardew Valley ${catalog.game_version}.`
          : "Your game's version could not be read, so which releases support it is unknown."}{" "}
        {catalog.source === "online"
          ? "Releases were just read from SMAPI's GitHub page."
          : catalog.source === "cached"
            ? `Releases as read from SMAPI's GitHub page${catalog.checked_at ? ` on ${new Date(catalog.checked_at).toLocaleString()}` : ""}.`
            : "Only the release this manager ships with is known; SMAPI's release list could not be read."}{" "}
        <button
          type="button"
          className="underline cursor-pointer"
          disabled={refreshing}
          onClick={() => void refresh()}
        >
          {refreshing ? "Checking..." : "Check again"}
        </button>
      </p>
      {catalog.error && (
        <p className="text-[var(--warning)]">{catalog.error}</p>
      )}
      <div className="flex flex-wrap gap-3">
        <label className="flex items-center gap-1">
          <input
            type="checkbox"
            checked={showAll}
            onChange={(e) => setShowAll(e.target.checked)}
          />
          Show versions for other game versions
        </label>
        <label className="flex items-center gap-1">
          <input
            type="checkbox"
            checked={showPre}
            onChange={(e) => setShowPre(e.target.checked)}
          />
          Show pre-releases
        </label>
      </div>
      <ul
        className="max-h-72 overflow-y-auto divide-y divide-[var(--border)] border border-[var(--border)] rounded-md"
        aria-label="SMAPI versions"
      >
        {visible.map((release) => {
          const fit = FIT[release.compatibility] ?? FIT.unknown;
          return (
            <li key={release.version} className="p-2">
              <label className="flex items-start gap-2 cursor-pointer">
                <input
                  type="radio"
                  name="smapi-version"
                  checked={selected === release.version}
                  onChange={() => onSelect(release.version)}
                />
                <span className="space-y-0.5">
                  <span className="flex flex-wrap items-center gap-1.5">
                    <span className="font-semibold">
                      SMAPI {release.version}
                    </span>
                    {release.is_recommended && (
                      <StatusBadge variant="success">Suggested</StatusBadge>
                    )}
                    {release.is_installed && (
                      <StatusBadge variant="info">Installed</StatusBadge>
                    )}
                    {release.prerelease && (
                      <StatusBadge variant="warning">Pre-release</StatusBadge>
                    )}
                    <StatusBadge variant={fit.variant}>{fit.label}</StatusBadge>
                  </span>
                  <span className="block text-[var(--fg-muted)]">
                    {range(release)}
                    {release.published_at
                      ? ` · released ${new Date(release.published_at).toLocaleDateString()}`
                      : ""}{" "}
                    · {CHECKSUM[release.checksum] ?? release.checksum}
                    {release.installer_kept ? " · installer kept here" : ""}
                  </span>
                  {release.notes_url && (
                    <span className="block break-all text-[var(--fg-muted)]">
                      Release notes: {release.notes_url}
                    </span>
                  )}
                </span>
              </label>
            </li>
          );
        })}
      </ul>
      {hidden > 0 && !showAll && (
        <p className="text-[var(--fg-muted)]">
          {hidden} other release(s) are hidden because they do not support your
          game or are pre-releases.
        </p>
      )}
    </div>
  );
};
