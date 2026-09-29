import React, { useMemo, useRef, useState } from "react";
import { Card } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { StatusBadge } from "@/components/ui/StatusBadge";
import { useActiveProfileOverview, useProfileMods } from "@/shared/api/hooks";
import {
  buildRecipe,
  compareWithRecipe,
  differenceKey,
  differenceLabel,
  parseRecipe,
  renderComparison,
  serializeRecipe,
  type ProfileRecipe,
} from "@/shared/recipe/recipe";
import { copyText, downloadText } from "@/shared/support/actions";
import { Download, FileUp, Share2 } from "lucide-react";

/** Largest recipe file read into memory. */
const MAX_RECIPE_BYTES = 2 * 1024 * 1024;

/**
 * Exports the active profile as a portable recipe and compares another recipe
 * with it. Comparison is read-only: nothing is installed, removed or changed.
 */
export const RecipeCard: React.FC = () => {
  const { data: overview } = useActiveProfileOverview();
  const { data: mods } = useProfileMods(overview?.profile.id);
  const fileInput = useRef<HTMLInputElement>(null);
  const [recipe, setRecipe] = useState<ProfileRecipe | null>(null);
  const [fileName, setFileName] = useState("");
  const [errors, setErrors] = useState<string[]>([]);
  const [accepted, setAccepted] = useState<ReadonlySet<string>>(new Set());
  const [status, setStatus] = useState<string | null>(null);

  const comparison = useMemo(
    () => (recipe && mods ? compareWithRecipe(mods, recipe) : null),
    [recipe, mods],
  );

  const handleExport = () => {
    if (!overview || !mods) return;
    downloadText(
      "profile-recipe.json",
      serializeRecipe(buildRecipe(overview, mods, new Date().toISOString())),
    );
  };

  const handleFile = async (file: File | undefined) => {
    setStatus(null);
    setAccepted(new Set());
    if (!file) return;
    if (file.size > MAX_RECIPE_BYTES) {
      setRecipe(null);
      setErrors(["That file is too large to be a profile recipe."]);
      return;
    }
    const result = parseRecipe(await file.text());
    setFileName(file.name);
    if (result.ok) {
      setRecipe(result.recipe);
      setErrors([]);
    } else {
      setRecipe(null);
      setErrors(result.errors);
    }
  };

  const toggleAccepted = (key: string) => {
    const next = new Set(accepted);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    setAccepted(next);
  };

  const unresolved = comparison
    ? comparison.differences.filter((d) => !accepted.has(differenceKey(d)))
    : [];

  return (
    <Card className="space-y-4">
      <div className="flex items-center gap-2 border-b border-[var(--border)] pb-3">
        <Share2 className="w-4 h-4 text-[var(--accent-primary)]" />
        <h3 className="font-bold text-sm">Profile recipes</h3>
      </div>
      <p className="text-xs text-[var(--fg-muted)] leading-relaxed">
        A recipe lists the mods in the active profile by UniqueID, exact version
        and package checksum, with no local paths. Share it, or compare someone
        else's recipe with this profile. Comparing changes nothing.
      </p>

      <div className="flex flex-wrap gap-2">
        <Button
          size="sm"
          variant="secondary"
          onClick={handleExport}
          disabled={!overview || !mods}
          className="flex items-center gap-1.5"
        >
          <Download className="w-3.5 h-3.5" />
          <span>Export recipe</span>
        </Button>
        <Button
          size="sm"
          variant="secondary"
          onClick={() => fileInput.current?.click()}
          className="flex items-center gap-1.5"
        >
          <FileUp className="w-3.5 h-3.5" />
          <span>Compare with recipe...</span>
        </Button>
        <input
          ref={fileInput}
          type="file"
          accept=".json,application/json"
          className="sr-only"
          aria-label="Recipe file"
          onChange={(event) => {
            void handleFile(event.target.files?.[0]);
            event.target.value = "";
          }}
        />
      </div>

      {errors.length > 0 && (
        <ul role="alert" className="text-xs text-[var(--danger)] space-y-1">
          {errors.map((message) => (
            <li key={message}>{message}</li>
          ))}
        </ul>
      )}

      {recipe && comparison && (
        <div className="space-y-3">
          <div className="flex flex-wrap items-center gap-2 text-xs">
            <span className="font-semibold">
              {recipe.profile_name || fileName}
            </span>
            <StatusBadge variant={comparison.identical ? "success" : "warning"}>
              {comparison.identical
                ? "Matches"
                : `${unresolved.length} to review`}
            </StatusBadge>
            <span className="text-[var(--fg-muted)]">
              {comparison.matching} matching
            </span>
          </div>

          {comparison.duplicates.length > 0 && (
            <p className="text-xs text-[var(--warning)]">
              Duplicate UniqueIDs make this comparison ambiguous:{" "}
              {comparison.duplicates.join(", ")}
            </p>
          )}

          {comparison.differences.length > 0 && (
            <ul className="divide-y divide-[var(--border)] border border-[var(--border)] rounded-lg">
              {comparison.differences.map((difference) => {
                const key = differenceKey(difference);
                return (
                  <li
                    key={key}
                    className="p-3 flex items-start justify-between gap-3 text-xs"
                  >
                    <div className="min-w-0">
                      <p className="font-semibold">
                        {differenceLabel(difference.kind)}
                        {difference.optional ? " (optional)" : ""}:{" "}
                        <span className="font-mono">
                          {difference.unique_id}
                        </span>
                      </p>
                      <p className="text-[var(--fg-muted)]">
                        {difference.detail}
                      </p>
                    </div>
                    <label className="flex items-center gap-1.5 shrink-0 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={accepted.has(key)}
                        onChange={() => toggleAccepted(key)}
                      />
                      <span>Acceptable</span>
                    </label>
                  </li>
                );
              })}
            </ul>
          )}

          {comparison.differences.some((d) => d.kind === "missing") && (
            <p className="text-xs text-[var(--fg-muted)]">
              A recipe does not contain the mods themselves. Install each
              missing mod from its own source, matching the version shown.
            </p>
          )}

          <Button
            size="sm"
            variant="ghost"
            onClick={async () =>
              setStatus(
                (await copyText(renderComparison(comparison, accepted)))
                  ? "Comparison copied."
                  : "Could not access the clipboard.",
              )
            }
          >
            Copy comparison
          </Button>
          {status && (
            <span role="status" className="text-xs text-[var(--fg-muted)] ml-2">
              {status}
            </span>
          )}
        </div>
      )}
    </Card>
  );
};
