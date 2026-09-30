import React, { useState } from "react";
import { Card } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { StatusBadge } from "@/components/ui/StatusBadge";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import type {
  CleanupItemDto,
  CleanupPreviewDto,
  CleanupResultDto,
} from "@/shared/api/generated";
import { HardDrive } from "lucide-react";

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(value < 10 ? 1 : 0)} ${units[unit]}`;
}

const CATEGORIES: { id: string; title: string; hint: string }[] = [
  {
    id: "unused_package",
    title: "Packages no profile uses",
    hint: "Mod archives kept after their mods were removed.",
  },
  {
    id: "cache",
    title: "Downloaded installers",
    hint: "Fetched again when needed.",
  },
  {
    id: "operation_leftover",
    title: "Leftovers from finished changes",
    hint: "Prepared files and undo copies nothing will use again.",
  },
];

const total = (items: CleanupItemDto[]) =>
  items.reduce((sum, item) => sum + item.size_bytes, 0);

/**
 * Reclaims space in the manager's own storage. The backend decides what is
 * safe to remove; this card only chooses among the items it offers.
 */
export const StorageCleanupCard: React.FC = () => {
  const [preview, setPreview] = useState<CleanupPreviewDto | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [result, setResult] = useState<CleanupResultDto | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const scan = async () => {
    setBusy(true);
    setError(null);
    try {
      const next = await api.getCleanupPreview();
      setPreview(next);
      // Every removable category starts selected; the user can opt out.
      setSelected(
        new Set(next.items.filter((i) => i.removable).map((i) => i.category)),
      );
    } catch (scanError) {
      setError(errorSummary(scanError, "Could not check storage"));
    } finally {
      setBusy(false);
    }
  };

  const chosen =
    preview?.items.filter((i) => i.removable && selected.has(i.category)) ?? [];

  const run = async () => {
    if (chosen.length === 0) return;
    if (
      !window.confirm(
        `Remove ${chosen.length} item(s) and free ${formatBytes(total(chosen))}? This cannot be undone.`,
      )
    )
      return;
    setBusy(true);
    setError(null);
    try {
      setResult(await api.runCleanup(chosen.map((i) => i.id)));
      setPreview(await api.getCleanupPreview());
      setSelected(new Set());
    } catch (runError) {
      setError(errorSummary(runError, "Cleanup did not run"));
    } finally {
      setBusy(false);
    }
  };

  const protectedItems = preview?.items.filter((i) => !i.removable) ?? [];
  const failed = result?.outcomes.filter((o) => o.outcome !== "removed") ?? [];

  return (
    <Card className="space-y-4">
      <div className="flex items-center gap-2 border-b border-[var(--border)] pb-3">
        <HardDrive className="w-4 h-4 text-[var(--accent-primary)]" />
        <h3 className="font-bold text-sm">Storage cleanup</h3>
      </div>
      <p className="text-xs text-[var(--fg-muted)] leading-relaxed">
        Frees space in the manager's own folders. Packages a profile uses
        (archived ones included), anything an unfinished change may need, and
        links are always kept. Your game folder and saves are never touched.
      </p>

      <Button size="sm" variant="secondary" onClick={scan} isLoading={busy}>
        {preview ? "Check again" : "Check storage"}
      </Button>

      {error && (
        <p role="alert" className="text-xs text-[var(--danger)]">
          {error}
        </p>
      )}

      {result && (
        <div role="status" className="text-xs space-y-1">
          <p>
            {result.complete
              ? `Freed ${formatBytes(result.reclaimed_bytes)}.`
              : `Freed ${formatBytes(result.reclaimed_bytes)}; ${failed.length} item(s) were not removed.`}
          </p>
          {failed.length > 0 && (
            <ul className="list-disc pl-4 text-[var(--fg-muted)]">
              {failed.map((o) => (
                <li key={o.id}>
                  {o.label}: {o.message ?? o.outcome}
                </li>
              ))}
            </ul>
          )}
        </div>
      )}

      {preview && (
        <div className="space-y-3">
          {preview.blocked_reason && (
            <p className="text-xs text-[var(--warning)]">
              {preview.blocked_reason}
            </p>
          )}
          {preview.items.length === 0 ? (
            <p className="text-xs text-[var(--fg-muted)]">
              Nothing to clean up: the manager is not keeping any extra files.
            </p>
          ) : (
            <>
              <fieldset className="space-y-2">
                <legend className="text-xs font-semibold mb-1">
                  Can be removed: {formatBytes(preview.reclaimable_bytes)}
                </legend>
                {CATEGORIES.map((category) => {
                  const items = preview.items.filter(
                    (i) => i.removable && i.category === category.id,
                  );
                  if (items.length === 0) return null;
                  return (
                    <details
                      key={category.id}
                      className="border border-[var(--border)] rounded-lg p-2 text-xs"
                    >
                      <summary className="flex items-center gap-2 cursor-pointer">
                        <input
                          type="checkbox"
                          aria-label={category.title}
                          checked={selected.has(category.id)}
                          onClick={(event) => event.stopPropagation()}
                          onChange={(event) => {
                            const next = new Set(selected);
                            if (event.target.checked) next.add(category.id);
                            else next.delete(category.id);
                            setSelected(next);
                          }}
                        />
                        <span className="font-medium">{category.title}</span>
                        <span className="text-[var(--fg-muted)]">
                          {items.length} item(s), {formatBytes(total(items))}
                        </span>
                      </summary>
                      <p className="mt-1 text-[var(--fg-muted)]">
                        {category.hint}
                      </p>
                      <ul className="mt-1 space-y-0.5">
                        {items.map((item) => (
                          <li key={item.id} title={item.detail}>
                            {item.label} ({formatBytes(item.size_bytes)})
                          </li>
                        ))}
                      </ul>
                    </details>
                  );
                })}
              </fieldset>

              {protectedItems.length > 0 && (
                <details className="border border-[var(--border)] rounded-lg p-2 text-xs">
                  <summary className="cursor-pointer">
                    <StatusBadge variant="neutral">Kept</StatusBadge>{" "}
                    {protectedItems.length} item(s),{" "}
                    {formatBytes(preview.protected_bytes)}
                  </summary>
                  <ul className="mt-1 space-y-1">
                    {protectedItems.map((item) => (
                      <li key={item.id}>
                        <span className="font-medium">{item.label}</span>{" "}
                        <span className="text-[var(--fg-muted)]">
                          {item.detail}
                        </span>
                      </li>
                    ))}
                  </ul>
                </details>
              )}

              <Button
                size="sm"
                variant="primary"
                disabled={busy || chosen.length === 0}
                onClick={run}
              >
                Remove selected ({formatBytes(total(chosen))})
              </Button>
            </>
          )}
        </div>
      )}
    </Card>
  );
};
