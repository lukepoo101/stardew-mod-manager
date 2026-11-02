import React from "react";
import { Button } from "@/components/ui/Button";
import { StatusBadge } from "@/components/ui/StatusBadge";
import type {
  ModAnnotationDto,
  ModFilesCheckDto,
  ModListItemDto,
} from "@/shared/api/generated";
import type { SkipMatch } from "@/shared/diagnostics/skipped";
import { Copy, Info, Star, Trash2 } from "lucide-react";

/** Built-in types from the manifest, shown apart from user tags. */
export const KIND_LABELS: Record<string, string> = {
  smapi_mod: "SMAPI mod",
  content_pack: "Content pack",
  other: "Other",
};

/**
 * What a row can ask the page to do. The page passes one object whose
 * identity never changes, so ticking one row re-renders that row only.
 */
export interface ModRowActions {
  /** `range` extends the selection from the last ticked row (Shift+click). */
  check: (id: string, index: number, range: boolean) => void;
  favourite: (mod: ModListItemDto) => void;
  copyId: (uniqueId: string) => void;
  toggle: (mod: ModListItemDto) => void;
  details: (profileComponentId: string) => void;
  remove: (mod: ModListItemDto) => void;
}

export interface ModRowProps {
  mod: ModListItemDto;
  /** Position in the shown list, for range selection. */
  index: number;
  checked: boolean;
  annotation: ModAnnotationDto | undefined;
  /** Unmet requirements, when there are any. */
  problem: string[] | undefined;
  skipped: Extract<SkipMatch, { mod: ModListItemDto }> | undefined;
  changed: ModFilesCheckDto | undefined;
  toggling: boolean;
  removing: boolean;
  actions: ModRowActions;
}

/**
 * One installed mod in the Mods list.
 *
 * A disabled mod is marked by its badge and a tinted background, never by
 * fading the row: faded text fell below readable contrast in both themes.
 */
export const ModRow = React.memo(function ModRow({
  mod,
  index,
  checked,
  annotation,
  problem,
  skipped,
  changed,
  toggling,
  removing,
  actions,
}: ModRowProps) {
  return (
    <li
      aria-label={mod.name}
      data-row
      className={`p-4 flex items-center justify-between gap-4 transition-colors ${
        mod.enabled
          ? "hover:bg-[var(--bg-elevated)]/40"
          : "bg-[var(--bg-disabled-row)] border-l-4 border-l-[var(--border-strong)]"
      }`}
    >
      <input
        type="checkbox"
        aria-label={`Select ${mod.name}`}
        title="Select. Shift+click selects every row from the last one ticked."
        checked={checked}
        onChange={(event) =>
          actions.check(
            mod.profile_component_id,
            index,
            Boolean((event.nativeEvent as MouseEvent).shiftKey),
          )
        }
        className="shrink-0"
      />
      {/* Left: Info */}
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2 flex-wrap">
          {mod.unique_id && (
            <button
              type="button"
              onClick={() => actions.favourite(mod)}
              aria-pressed={Boolean(annotation?.favourite)}
              aria-label={`Favourite ${mod.name}`}
              title="Favourite"
              className="cursor-pointer text-[var(--fg-muted)] hover:text-amber-500"
            >
              <Star
                className={`w-4 h-4 ${
                  annotation?.favourite ? "fill-amber-400 text-amber-500" : ""
                }`}
              />
            </button>
          )}
          <h4 className="font-bold text-sm text-[var(--fg-primary)] min-w-0 break-words">
            {mod.name}
          </h4>
          <span className="text-xs px-2 py-0.5 rounded bg-[var(--bg-elevated)] border border-[var(--border)] font-mono text-[var(--fg-muted)]">
            v{mod.version}
          </span>
          <span className="text-xs text-[var(--fg-muted)]">
            by {mod.author}
          </span>
          {mod.enabled ? (
            <StatusBadge variant="success">Enabled</StatusBadge>
          ) : (
            <StatusBadge variant="neutral">Disabled</StatusBadge>
          )}
          {problem && (
            <span title={`Needs ${problem.join(", ")}`}>
              <StatusBadge variant="danger">Missing requirement</StatusBadge>
            </span>
          )}
          {mod.folder_missing && (
            <span title="Its folder is not where the manager put it. Check mod files on the Diagnostics page, or reinstall it.">
              <StatusBadge variant="danger">Folder missing</StatusBadge>
            </span>
          )}
          {changed && (
            <span
              title={`Changed outside the manager (by file size): ${[
                ...changed.missing.map((p) => `${p} missing`),
                ...changed.modified.map((p) => `${p} changed`),
              ].join(
                ", ",
              )}. Check mod files on the Diagnostics page to confirm and review.`}
            >
              <StatusBadge variant="warning">Files changed</StatusBadge>
            </span>
          )}
          {skipped && (
            <span
              title={`${skipped.skipped.reason ?? ""} (SMAPI log line ${skipped.skipped.line})`}
            >
              <StatusBadge variant="warning">
                {skipped.kind === "exact"
                  ? "Skipped last session"
                  : "Possibly skipped last session"}
              </StatusBadge>
            </span>
          )}
        </div>
        <p className="text-xs font-mono text-[var(--fg-muted)] mt-1 truncate">
          {mod.unique_id || "UniqueID unavailable (manifest invalid)"}
        </p>
        {mod.description && (
          <p className="text-xs text-[var(--fg-muted)] mt-1 line-clamp-1">
            {mod.description}
          </p>
        )}
        {KIND_LABELS[mod.kind] && (
          <p
            className="text-[10px] uppercase tracking-wider text-[var(--fg-muted)] mt-1"
            title="From the mod's manifest, not a tag"
          >
            {KIND_LABELS[mod.kind]}
          </p>
        )}
        {(annotation?.tags.length ?? 0) > 0 && (
          <ul className="flex flex-wrap gap-1 mt-1" aria-label="Tags">
            {annotation?.tags.map((tag) => (
              <li
                key={tag}
                className="text-[10px] px-1.5 py-0.5 rounded-full bg-[var(--bg-elevated)] border border-[var(--border)]"
              >
                {tag}
              </li>
            ))}
          </ul>
        )}
      </div>

      {/* Right: Actions */}
      <div className="flex items-center gap-2 shrink-0">
        {mod.unique_id && (
          <Button
            variant="ghost"
            size="sm"
            onClick={() => actions.copyId(mod.unique_id)}
            title="Copy UniqueID"
            aria-label={`Copy UniqueID ${mod.unique_id}`}
          >
            <Copy className="w-4 h-4" />
          </Button>
        )}
        {/* Busy rather than disabled while it runs: a disabled button
            drops keyboard focus to the page, and the user is on this row. */}
        <Button
          variant="secondary"
          size="sm"
          aria-disabled={toggling || undefined}
          aria-busy={toggling || undefined}
          onClick={() => {
            if (!toggling) actions.toggle(mod);
          }}
          aria-label={`${mod.enabled ? "Disable" : "Enable"} ${mod.name}`}
          title={
            mod.enabled
              ? "Stop SMAPI from loading this mod"
              : "Let SMAPI load this mod again"
          }
        >
          {mod.enabled ? "Disable" : "Enable"}
        </Button>
        <Button
          variant="ghost"
          size="sm"
          onClick={() => actions.details(mod.profile_component_id)}
          title="View manifest details"
          aria-label={`Details for ${mod.name}`}
        >
          <Info className="w-4 h-4" />
        </Button>

        <button
          type="button"
          onClick={() => actions.remove(mod)}
          aria-label={`Remove ${mod.name}`}
          disabled={removing}
          className="p-2 hover:bg-[var(--danger-surface)] rounded text-[var(--fg-muted)] hover:text-[var(--danger)] cursor-pointer transition-colors"
          title="Remove mod"
        >
          <Trash2 className="w-4 h-4" />
        </button>
      </div>
    </li>
  );
});
