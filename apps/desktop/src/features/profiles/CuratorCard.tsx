import React, { useMemo, useRef, useState } from "react";
import { Card } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { StatusBadge } from "@/components/ui/StatusBadge";
import { useActiveProfileOverview, useProfileMods } from "@/shared/api/hooks";
import {
  buildRecipe,
  parseRecipe,
  type ProfileRecipe,
} from "@/shared/recipe/recipe";
import {
  checkRecipe,
  diffRecipes,
  isEmptyChangelog,
  renderChangelog,
} from "@/shared/recipe/curator";
import { copyText } from "@/shared/support/actions";
import { ClipboardCheck } from "lucide-react";

const MAX_BYTES = 2 * 1024 * 1024;

async function readRecipe(file: File): Promise<ProfileRecipe | string[]> {
  if (file.size > MAX_BYTES) return ["That file is too large to be a recipe."];
  const parsed = parseRecipe(await file.text());
  return parsed.ok ? parsed.recipe : parsed.errors;
}

/**
 * Checks a recipe before it is shared and writes the changelog between two
 * revisions. It only reads: nothing is installed, removed or published.
 */
export const CuratorCard: React.FC = () => {
  const { data: overview } = useActiveProfileOverview();
  const { data: mods } = useProfileMods(overview?.profile.id);
  const previousInput = useRef<HTMLInputElement>(null);
  const [previous, setPrevious] = useState<ProfileRecipe | null>(null);
  const [errors, setErrors] = useState<string[]>([]);
  const [status, setStatus] = useState<string | null>(null);

  // The recipe being prepared for sharing is the active profile as it is now.
  const current = useMemo(
    () =>
      overview && mods
        ? buildRecipe(overview, mods, new Date().toISOString())
        : null,
    [overview, mods],
  );
  const report = current ? checkRecipe(current) : null;
  const changelog = current && previous ? diffRecipes(previous, current) : null;
  const notes =
    current && previous && changelog
      ? renderChangelog(previous, current, changelog)
      : null;

  return (
    <Card className="space-y-4">
      <div className="flex items-center gap-2 border-b border-[var(--border)] pb-3">
        <ClipboardCheck className="w-4 h-4 text-[var(--accent-primary)]" />
        <h3 className="font-bold text-sm">Before you share this profile</h3>
      </div>
      <p className="text-xs text-[var(--fg-muted)] leading-relaxed">
        These checks say how exactly someone else can reproduce this profile.
        They do not say whether any mod is safe or will work for them.
      </p>

      {report && (
        <div className="space-y-2">
          <div className="flex flex-wrap items-center gap-2 text-xs">
            <StatusBadge variant={report.publishable ? "success" : "danger"}>
              {report.publishable ? "Ready to share" : "Needs attention"}
            </StatusBadge>
            <span>
              Reproducibility: <strong>{report.reproducibility}%</strong> of
              mods have an exact version and package checksum
            </span>
          </div>
          <ul className="divide-y divide-[var(--border)] border border-[var(--border)] rounded-lg text-xs">
            {report.checks.map((check) => (
              <li key={check.id} className="p-2 flex items-start gap-2">
                <StatusBadge
                  variant={
                    check.status === "pass"
                      ? "success"
                      : check.status === "warn"
                        ? "warning"
                        : "danger"
                  }
                >
                  {check.status === "pass"
                    ? "Pass"
                    : check.status === "warn"
                      ? "Check"
                      : "Fix"}
                </StatusBadge>
                <span>
                  <span className="font-medium">{check.label}.</span>{" "}
                  <span className="text-[var(--fg-muted)]">{check.detail}</span>
                </span>
              </li>
            ))}
          </ul>
        </div>
      )}

      <div className="space-y-2">
        <h4 className="text-xs font-semibold">
          Changes since an earlier recipe
        </h4>
        <Button
          size="sm"
          variant="secondary"
          onClick={() => previousInput.current?.click()}
        >
          Choose the earlier recipe...
        </Button>
        <input
          ref={previousInput}
          type="file"
          accept=".json,application/json"
          className="sr-only"
          aria-label="Earlier recipe file"
          onChange={async (event) => {
            const file = event.target.files?.[0];
            event.target.value = "";
            setStatus(null);
            if (!file) return;
            const result = await readRecipe(file);
            if (Array.isArray(result)) {
              setPrevious(null);
              setErrors(result);
            } else {
              setPrevious(result);
              setErrors([]);
            }
          }}
        />
        {errors.length > 0 && (
          <ul role="alert" className="text-xs text-[var(--danger)]">
            {errors.map((message) => (
              <li key={message}>{message}</li>
            ))}
          </ul>
        )}
        {notes && changelog && (
          <div className="space-y-2">
            <textarea
              readOnly
              aria-label="Changelog"
              rows={Math.min(14, notes.split("\n").length + 1)}
              value={notes}
              className="w-full p-3 rounded-lg bg-[var(--bg-primary)] border border-[var(--border)] text-xs font-mono"
            />
            <div className="flex items-center gap-2">
              <Button
                size="sm"
                variant="ghost"
                disabled={isEmptyChangelog(changelog)}
                onClick={async () =>
                  setStatus(
                    (await copyText(notes))
                      ? "Changelog copied."
                      : "Could not access the clipboard.",
                  )
                }
              >
                Copy changelog
              </Button>
              {status && (
                <span role="status" className="text-xs text-[var(--fg-muted)]">
                  {status}
                </span>
              )}
            </div>
          </div>
        )}
      </div>
    </Card>
  );
};
