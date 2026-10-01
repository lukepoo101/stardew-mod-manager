import React, { useState } from "react";
import { Card } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import { useActiveProfileOverview } from "@/shared/api/hooks";
import type {
  BundleExportDto,
  BundleImportDto,
  BundlePreviewDto,
  ShareableSettingsDto,
} from "@/shared/api/generated";
import { PackageOpen } from "lucide-react";

/**
 * Moves a profile between computers or people as one file that carries the mod
 * packages as well as the list. Importing always builds a new profile, so the
 * existing ones are never touched.
 */
export const BundleCard: React.FC = () => {
  const { data: overview } = useActiveProfileOverview();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [exported, setExported] = useState<BundleExportDto | null>(null);
  const [bundlePath, setBundlePath] = useState<string | null>(null);
  const [preview, setPreview] = useState<BundlePreviewDto | null>(null);
  const [name, setName] = useState("");
  const [result, setResult] = useState<BundleImportDto | null>(null);
  const [shareable, setShareable] = useState<ShareableSettingsDto[] | null>(
    null,
  );
  const [chosen, setChosen] = useState<ReadonlySet<string>>(new Set());

  const guard = async (work: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await work();
    } catch (failure) {
      setError(errorSummary(failure, "That did not work"));
    } finally {
      setBusy(false);
    }
  };

  const exportBundle = () =>
    guard(async () => {
      const folder = await api.pickFolderDialog();
      if (!folder) return;
      setResult(null);
      setExported(await api.exportProfileBundle(folder, [...chosen]));
    });

  const loadShareable = () =>
    guard(async () => {
      if (!overview) return;
      setShareable(await api.listShareableSettings(overview.profile.id));
    });

  const chooseBundle = () =>
    guard(async () => {
      const path = await api.pickArchiveDialog();
      if (!path) return;
      const found = await api.inspectProfileBundle(path);
      setBundlePath(path);
      setPreview(found);
      setName(found.profile_name);
      setExported(null);
      setResult(null);
    });

  const importBundle = () =>
    guard(async () => {
      if (!bundlePath || !overview?.game.id) return;
      setResult(
        await api.importProfileBundle(bundlePath, overview.game.id, name),
      );
      setPreview(null);
      setBundlePath(null);
    });

  return (
    <Card className="space-y-4">
      <div className="flex items-center gap-2 border-b border-[var(--border)] pb-3">
        <PackageOpen className="w-4 h-4 text-[var(--accent-primary)]" />
        <h3 className="font-bold text-sm">Move a profile with its mods</h3>
      </div>
      <p className="text-xs text-[var(--fg-muted)] leading-relaxed">
        A bundle is one file holding this profile's mod list and the mod
        packages themselves. Importing one creates a new profile beside your
        existing ones and installs each mod into it. Only import bundles from
        people you trust: the mods inside run with your account's permissions.
      </p>

      <details
        className="text-xs"
        onToggle={(event) => {
          if ((event.target as HTMLDetailsElement).open && !shareable)
            void loadShareable();
        }}
      >
        <summary className="cursor-pointer">
          Include mod settings ({chosen.size} chosen)
        </summary>
        <p className="mt-1 text-[var(--fg-muted)]">
          Settings are left out unless you choose them. Some mods keep keys,
          account names or paths from your computer in their settings; anything
          that looks like that is pointed out below, but check before sharing.
        </p>
        {shareable && shareable.length === 0 && (
          <p className="mt-1">
            No mod in this profile has settings to include.
          </p>
        )}
        {shareable && shareable.length > 0 && (
          <ul className="mt-1 space-y-1">
            {shareable.map((entry) => (
              <li key={entry.unique_id}>
                <label className="flex items-start gap-2">
                  <input
                    type="checkbox"
                    checked={chosen.has(entry.unique_id)}
                    onChange={(event) => {
                      const next = new Set(chosen);
                      if (event.target.checked) next.add(entry.unique_id);
                      else next.delete(entry.unique_id);
                      setChosen(next);
                    }}
                  />
                  <span>
                    {entry.name}{" "}
                    <span className="text-[var(--fg-muted)]">
                      ({entry.files.join(", ")})
                    </span>
                    {entry.warnings.map((warning) => (
                      <span
                        key={warning}
                        className="block text-[var(--warning)]"
                      >
                        {warning}
                      </span>
                    ))}
                  </span>
                </label>
              </li>
            ))}
          </ul>
        )}
      </details>

      <div className="flex flex-wrap gap-2">
        <Button
          size="sm"
          variant="secondary"
          disabled={busy || !overview}
          onClick={exportBundle}
        >
          Export bundle...
        </Button>
        <Button
          size="sm"
          variant="secondary"
          disabled={busy}
          onClick={chooseBundle}
        >
          Import bundle...
        </Button>
      </div>

      {error && (
        <p role="alert" className="text-xs text-[var(--danger)]">
          {error}
        </p>
      )}

      {exported && (
        <div role="status" className="text-xs space-y-1">
          <p>
            Saved {exported.component_count} mod(s) ({exported.package_count}{" "}
            package(s)) to{" "}
            <span className="font-mono break-all">{exported.path}</span>
          </p>
          {exported.settings_included.length > 0 && (
            <p>
              Included settings for: {exported.settings_included.join(", ")}.
            </p>
          )}
          {exported.missing_packages.length > 0 && (
            <p className="text-[var(--warning)]">
              These mods are listed but their packages are no longer stored, so
              the bundle cannot install them:{" "}
              {exported.missing_packages.join(", ")}.
            </p>
          )}
        </div>
      )}

      {preview && (
        <div className="space-y-3 text-xs">
          <p>
            <span className="font-semibold">{preview.profile_name}</span> lists{" "}
            {preview.components.length} mod(s).
          </p>
          {preview.settings_for.length > 0 && (
            <p>
              It also carries settings for {preview.settings_for.length} mod(s)
              ({preview.settings_for.join(", ")}). They are written into those
              mods after they are installed.
            </p>
          )}
          <ul className="divide-y divide-[var(--border)] border border-[var(--border)] rounded-lg">
            {preview.components.map((component) => (
              <li
                key={`${component.unique_id}-${component.version}`}
                className="p-2 flex justify-between gap-2"
              >
                <span>
                  {component.name} {component.version}
                  {component.enabled ? "" : " (disabled)"}
                </span>
                {!component.package_included && (
                  <span className="text-[var(--warning)] shrink-0">
                    package not included
                  </span>
                )}
              </li>
            ))}
          </ul>
          {preview.warnings.map((warning) => (
            <p key={warning} className="text-[var(--warning)]">
              {warning}
            </p>
          ))}
          <label className="flex items-center gap-2">
            <span className="font-medium">New profile name</span>
            <input
              value={name}
              onChange={(event) => setName(event.target.value)}
              className="px-2 py-1 rounded-md border border-[var(--border)] bg-[var(--bg-surface)]"
            />
          </label>
          <div className="flex gap-2">
            <Button
              size="sm"
              variant="primary"
              disabled={busy || !name.trim()}
              isLoading={busy}
              onClick={importBundle}
            >
              Create profile and install
            </Button>
            <Button
              size="sm"
              variant="secondary"
              disabled={busy}
              onClick={() => {
                setPreview(null);
                setBundlePath(null);
              }}
            >
              Cancel
            </Button>
          </div>
        </div>
      )}

      {result && (
        <div role="status" className="text-xs space-y-1">
          <p>
            Created <span className="font-semibold">{result.profile_name}</span>{" "}
            with {result.installed.length} mod(s) installed.
            {result.disabled.length > 0 &&
              ` Left disabled, as in the original: ${result.disabled.join(", ")}.`}
          </p>
          {result.failures.length > 0 && (
            <div className="text-[var(--danger)]">
              <p>Some mods could not be installed:</p>
              <ul className="list-disc pl-4">
                {result.failures.map((failure) => (
                  <li key={failure.name}>
                    {failure.name}: {failure.reason}
                  </li>
                ))}
              </ul>
            </div>
          )}
          <p className="text-[var(--fg-muted)]">
            Switch to the new profile from the list above when you are ready.
          </p>
        </div>
      )}
    </Card>
  );
};
