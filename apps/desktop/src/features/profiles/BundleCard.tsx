import React, { useEffect, useId, useState } from "react";
import { Card } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { api } from "@/shared/api/client";
import { errorSummary, normalizeApiError } from "@/shared/api/errors";
import {
  useActiveProfileOverview,
  useProfileMods,
  useProfiles,
} from "@/shared/api/hooks";
import type {
  BundleExportDto,
  BundleImportDto,
  BundlePreviewDto,
  BundleProgressDto,
  ShareableSettingsDto,
} from "@/shared/api/generated";
import { PackageOpen } from "lucide-react";
import { folderOrAsk, ShareFolderField, useShareFolder } from "./ShareFolder";

const SAVE_TITLE = "Choose where to save the bundle";

/** A name for the new profile that is not taken yet. */
export function freeProfileName(
  wanted: string,
  taken: readonly string[],
): string {
  const used = new Set(taken.map((n) => n.trim().toLowerCase()));
  if (!used.has(wanted.trim().toLowerCase())) return wanted;
  for (let i = 1; i < 100; i += 1) {
    const candidate =
      i === 1 ? `${wanted} (from bundle)` : `${wanted} (from bundle ${i})`;
    if (!used.has(candidate.toLowerCase())) return candidate;
  }
  return wanted;
}

/**
 * Moves a profile between computers or people as one file that carries the mod
 * packages as well as the list. Importing always builds a new profile, so the
 * existing ones are never touched.
 */
export const BundleCard: React.FC = () => {
  const { data: overview } = useActiveProfileOverview();
  const { data: profiles } = useProfiles();
  const [busy, setBusy] = useState(false);
  const [importing, setImporting] = useState(false);
  const [progress, setProgress] = useState<BundleProgressDto | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [nameError, setNameError] = useState<string | null>(null);
  const [exported, setExported] = useState<BundleExportDto | null>(null);
  const [folder, setFolder] = useShareFolder();
  const [typedPath, setTypedPath] = useState("");
  const [bundlePath, setBundlePath] = useState<string | null>(null);
  const [preview, setPreview] = useState<BundlePreviewDto | null>(null);
  const [name, setName] = useState("");
  const [result, setResult] = useState<BundleImportDto | null>(null);
  // Names of the imported bundle's mods, by lower-case UniqueID.
  const [importedNames, setImportedNames] = useState<
    ReadonlyMap<string, string>
  >(new Map());
  const [shareable, setShareable] = useState<ShareableSettingsDto[] | null>(
    null,
  );
  const [chosen, setChosen] = useState<ReadonlySet<string>>(new Set());
  // Mods whose flagged settings fields (paths, emails, account details) the
  // user chose to share; keys, tokens and passwords never are.
  const [keepFlagged, setKeepFlagged] = useState<ReadonlySet<string>>(
    new Set(),
  );
  const { data: mods } = useProfileMods(overview?.profile.id);
  // Mods the recipient may skip, chosen by the author when exporting.
  const [optional, setOptional] = useState<ReadonlySet<string>>(new Set());
  // Optional mods the recipient chose to install; none until they choose.
  const [optionalChosen, setOptionalChosen] = useState<ReadonlySet<string>>(
    new Set(),
  );
  // Bundled settings the recipient will apply; all of them unless unticked.
  const [settingsChosen, setSettingsChosen] = useState<ReadonlySet<string>>(
    new Set(),
  );
  // Importing without some required mods needs an explicit yes.
  const [acceptIncomplete, setAcceptIncomplete] = useState(false);
  const [refreeze, setRefreeze] = useState(false);
  const nameId = useId();
  const pathId = useId();
  const toggleIn = (
    set: ReadonlySet<string>,
    update: (next: ReadonlySet<string>) => void,
    id: string,
    on: boolean,
  ) => {
    const next = new Set(set);
    if (on) next.add(id);
    else next.delete(id);
    update(next);
  };

  // Per-mod progress while an import runs; it runs off the UI thread, so
  // it can be asked how far it has got.
  useEffect(() => {
    if (!importing) {
      setProgress(null);
      return;
    }
    const timer = window.setInterval(() => {
      api
        .profileBundleProgress()
        .then(setProgress)
        .catch(() => undefined);
    }, 400);
    return () => window.clearInterval(timer);
  }, [importing]);

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
      const target = await folderOrAsk(folder, SAVE_TITLE, setFolder);
      if (!target) return;
      setResult(null);
      setExported(
        await api.exportProfileBundle(
          target,
          [...chosen],
          [...optional],
          [...keepFlagged].filter((id) => chosen.has(id)),
        ),
      );
    });

  const loadShareable = () =>
    guard(async () => {
      if (!overview) return;
      setShareable(await api.listShareableSettings(overview.profile.id));
    });

  /** Reads a bundle; a refused one leaves nothing that could be imported. */
  const readBundle = (path: string) =>
    guard(async () => {
      setPreview(null);
      setBundlePath(null);
      setResult(null);
      setExported(null);
      setNameError(null);
      const found = await api.inspectProfileBundle(path);
      setBundlePath(path);
      setTypedPath(path);
      setPreview(found);
      setOptionalChosen(new Set());
      setSettingsChosen(new Set(found.settings_for));
      setAcceptIncomplete(false);
      setRefreeze(Boolean(found.frozen_at));
      setName(
        freeProfileName(
          found.profile_name,
          (profiles ?? []).map((p) => p.name),
        ),
      );
    });

  const chooseBundle = async () => {
    setError(null);
    try {
      const path = await api.chooseBundleDialog();
      if (path) await readBundle(path);
    } catch (failure) {
      setError(errorSummary(failure, "That did not work"));
    }
  };

  const importBundle = () =>
    guard(async () => {
      if (!bundlePath || !overview?.game.id) return;
      setNameError(null);
      setImporting(true);
      try {
        const imported = await api.importProfileBundle(
          bundlePath,
          overview.game.id,
          name,
          [...optionalChosen],
          [...settingsChosen],
          preview?.bundle_digest,
        );
        // Keep the freeze the exported profile had, if asked.
        if (refreeze && preview?.frozen_at)
          await api.freezeProfile(
            imported.profile_id,
            preview.frozen_reason ?? "",
          );
        setImportedNames(
          new Map(
            (preview?.components ?? []).map((c) => [
              c.unique_id.toLowerCase(),
              c.name,
            ]),
          ),
        );
        setResult(imported);
        setPreview(null);
        setBundlePath(null);
      } catch (failure) {
        // A taken name is said next to the name, where it can be changed.
        const code = normalizeApiError(failure).code;
        if (code === "DUPLICATE_PROFILE_NAME") {
          setNameError(errorSummary(failure, "That name is taken"));
          return;
        }
        // The file changed since it was read: what was shown is not what
        // it holds, so the preview goes and it must be read again.
        if (code === "BUNDLE_CHANGED") {
          setPreview(null);
          setBundlePath(null);
        }
        throw failure;
      } finally {
        setImporting(false);
      }
    });

  const modName = (id: string) =>
    preview?.components.find(
      (c) => c.unique_id.toLowerCase() === id.toLowerCase(),
    )?.name ?? id;
  const notHere = preview?.components.filter(
    (c) => c.not_for_this_computer || c.left_out_reason,
  );
  const listed = (optionalOnes: boolean) =>
    (preview?.components ?? []).filter(
      (c) =>
        c.optional === optionalOnes &&
        !c.not_for_this_computer &&
        !c.left_out_reason,
    );
  /**
   * A package is installed, enabled and disabled as a whole, so an optional
   * mod in one package with a mod that is installed comes with it. Names the
   * mods that bring it, or none if it can still be left out.
   */
  const broughtBy = (component: {
    shares_package_with: string[];
    package_included: boolean;
  }) =>
    component.package_included
      ? component.shares_package_with.filter((name) => {
          const mate = preview?.components.find((c) => c.name === name);
          return (
            mate?.package_included &&
            !mate.not_for_this_computer &&
            !mate.left_out_reason &&
            (!mate.optional || optionalChosen.has(mate.unique_id))
          );
        })
      : [];

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
          Settings are left out unless you choose them. Fields named like keys,
          tokens or passwords are always taken out. Paths from your computer,
          email addresses and account details are taken out too, unless you
          choose to share them for that mod. The scan looks for common shapes
          and cannot recognise every secret, so check before sharing.
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
                    onChange={(event) =>
                      toggleIn(
                        chosen,
                        setChosen,
                        entry.unique_id,
                        event.target.checked,
                      )
                    }
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
                {chosen.has(entry.unique_id) && entry.warnings.length > 0 && (
                  <label className="ml-6 flex items-center gap-2">
                    <input
                      type="checkbox"
                      checked={keepFlagged.has(entry.unique_id)}
                      onChange={(event) =>
                        toggleIn(
                          keepFlagged,
                          setKeepFlagged,
                          entry.unique_id,
                          event.target.checked,
                        )
                      }
                    />
                    Share flagged paths, emails and account details from{" "}
                    {entry.name} too (fields named like keys, tokens or
                    passwords are still taken out)
                  </label>
                )}
              </li>
            ))}
          </ul>
        )}
      </details>

      <details className="text-xs">
        <summary className="cursor-pointer">
          Mark mods as optional ({optional.size} marked)
        </summary>
        <p className="mt-1 text-[var(--fg-muted)]">
          Optional mods are listed separately when someone imports the bundle,
          and are installed only if they choose them.
        </p>
        <ul className="mt-1 space-y-1">
          {(mods ?? []).map((mod) => (
            <li key={mod.profile_component_id}>
              <label className="flex items-center gap-2">
                <input
                  type="checkbox"
                  checked={optional.has(mod.unique_id)}
                  onChange={(event) =>
                    toggleIn(
                      optional,
                      setOptional,
                      mod.unique_id,
                      event.target.checked,
                    )
                  }
                />
                <span>
                  {mod.name} {mod.version}
                </span>
              </label>
            </li>
          ))}
        </ul>
      </details>

      <div className="space-y-2">
        <ShareFolderField
          label="Save bundles in"
          folder={folder}
          onChange={setFolder}
          dialogTitle={SAVE_TITLE}
          disabled={busy}
        />
        <Button
          size="sm"
          variant="secondary"
          disabled={busy || !overview}
          onClick={exportBundle}
        >
          Export bundle...
        </Button>
      </div>

      <div className="space-y-2 text-xs">
        <div className="flex flex-wrap items-center gap-2">
          <label htmlFor={pathId} className="font-medium">
            Bundle to import
          </label>
          <input
            id={pathId}
            value={typedPath}
            disabled={busy}
            placeholder="Path to a .smm-bundle.zip file"
            onChange={(event) => setTypedPath(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && typedPath.trim())
                void readBundle(typedPath.trim());
            }}
            className="flex-1 min-w-[12rem] px-2 py-1 rounded-md border border-[var(--border)] bg-[var(--bg-surface)] select-text"
          />
          <Button
            size="sm"
            variant="ghost"
            disabled={busy || !typedPath.trim()}
            onClick={() => void readBundle(typedPath.trim())}
          >
            Read bundle
          </Button>
        </div>
        <Button
          size="sm"
          variant="secondary"
          disabled={busy}
          onClick={() => void chooseBundle()}
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
            <span className="font-mono break-all select-text">
              {exported.path}
            </span>
          </p>
          {exported.settings_included.length > 0 && (
            <p>
              Included settings for: {exported.settings_included.join(", ")}.
            </p>
          )}
          {exported.settings_redacted.length > 0 && (
            <div>
              <p>Taken out of the shared settings:</p>
              <ul className="list-disc pl-4">
                {exported.settings_redacted.map((line) => (
                  <li key={line}>{line}</li>
                ))}
              </ul>
            </div>
          )}
          {exported.settings_skipped.length > 0 && (
            <p className="text-[var(--warning)]">
              Not carried, because only JSON settings files up to 256 KB can be:{" "}
              {exported.settings_skipped.join(", ")}.
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
          <ul className="divide-y divide-[var(--border)] border border-[var(--border)] rounded-lg">
            {listed(false).map((component) => (
              <li
                key={`${component.unique_id}-${component.version}`}
                className="p-2 flex justify-between gap-2"
              >
                <span>
                  {component.name} {component.version}
                  {component.enabled ? "" : " (disabled)"}
                  {component.shares_package_with.length > 0 && (
                    <span className="text-[var(--fg-muted)]">
                      {" "}
                      (one package with{" "}
                      {component.shares_package_with.join(", ")})
                    </span>
                  )}
                </span>
                {!component.package_included && (
                  <span className="text-[var(--warning)] shrink-0">
                    package not included
                  </span>
                )}
              </li>
            ))}
          </ul>
          {notHere && notHere.length > 0 && (
            <div>
              <p className="font-semibold">
                Not for this computer ({preview.operating_system}), so not
                installed
              </p>
              <ul className="list-disc pl-4">
                {notHere.map((c) => (
                  <li key={`${c.unique_id}-${c.version}`}>
                    {c.name} {c.version}
                    {c.left_out_reason
                      ? `: ${c.left_out_reason}`
                      : ` (works on ${c.platforms.join(", ")})`}
                  </li>
                ))}
              </ul>
            </div>
          )}
          {listed(true).length > 0 && (
            <fieldset className="space-y-1">
              <legend className="font-semibold">
                Optional mods (installed only if you choose them)
              </legend>
              {listed(true).map((component) => {
                const bringers = broughtBy(component);
                return (
                  <div key={`${component.unique_id}-${component.version}`}>
                    <label className="flex items-center gap-2">
                      <input
                        type="checkbox"
                        checked={
                          bringers.length > 0 ||
                          optionalChosen.has(component.unique_id)
                        }
                        disabled={bringers.length > 0}
                        onChange={(event) =>
                          toggleIn(
                            optionalChosen,
                            setOptionalChosen,
                            component.unique_id,
                            event.target.checked,
                          )
                        }
                      />
                      <span>
                        {component.name} {component.version}
                        {component.package_included
                          ? ""
                          : " (package not included)"}
                      </span>
                    </label>
                    {bringers.length > 0 ? (
                      <p className="ml-6 text-[var(--fg-muted)]">
                        It comes in one package with {bringers.join(", ")},
                        which is installed, so it cannot be left out.
                      </p>
                    ) : (
                      component.shares_package_with.length > 0 && (
                        <p className="ml-6 text-[var(--fg-muted)]">
                          It comes in one package with{" "}
                          {component.shares_package_with.join(", ")}: choosing
                          either installs both.
                        </p>
                      )
                    )}
                  </div>
                );
              })}
              <p className="text-[var(--fg-muted)]">
                If a mod you install needs an optional one you leave out, it is
                reported as not installed.
              </p>
            </fieldset>
          )}
          {preview.settings_for.length > 0 && (
            <fieldset className="space-y-1">
              <legend className="font-semibold">
                Settings that come with the bundle
              </legend>
              <p className="text-[var(--fg-muted)]">
                They are written into these mods after they are installed.
                Untick any you would rather set up yourself.
              </p>
              {preview.settings_for.map((id) => (
                <label key={id} className="flex items-center gap-2">
                  <input
                    type="checkbox"
                    checked={settingsChosen.has(id)}
                    onChange={(event) =>
                      toggleIn(
                        settingsChosen,
                        setSettingsChosen,
                        id,
                        event.target.checked,
                      )
                    }
                  />
                  Use the settings for {modName(id)}
                </label>
              ))}
            </fieldset>
          )}
          {preview.warnings.map((warning) => (
            <p key={warning} className="text-[var(--warning)]">
              {warning}
            </p>
          ))}
          {preview.unresolved_requirements.length > 0 && (
            <div className="text-[var(--warning)]">
              <p>
                Requirements the bundle does not provide, so these mods will not
                load until you add them:
              </p>
              <ul className="list-disc pl-4">
                {preview.unresolved_requirements.map((line) => (
                  <li key={line}>{line}</li>
                ))}
              </ul>
            </div>
          )}
          {preview.locally_modified.length > 0 && (
            <p>
              Changed outside the manager when exported:{" "}
              {preview.locally_modified.join(", ")}. You get the packages as
              published, not those changes.
            </p>
          )}
          {preview.frozen_at && (
            <label className="flex items-start gap-2">
              <input
                type="checkbox"
                checked={refreeze}
                onChange={(event) => setRefreeze(event.target.checked)}
              />
              <span>
                It was frozen ({new Date(preview.frozen_at).toLocaleString()}
                {preview.frozen_reason ? `: ${preview.frozen_reason}` : ""}).
                Freeze the new profile too.
              </span>
            </label>
          )}
          {preview.missing_packages.length > 0 && (
            <label className="flex items-start gap-2 text-[var(--warning)]">
              <input
                type="checkbox"
                checked={acceptIncomplete}
                onChange={(event) => setAcceptIncomplete(event.target.checked)}
              />
              <span>
                Create the profile without {preview.missing_packages.join(", ")}
                . They cannot be installed from this bundle (their package is
                not in it, or comes with a mod that is not for this computer),
                so they stay listed under Group reference until you get them.
              </span>
            </label>
          )}
          <div className="space-y-1">
            <div className="flex items-center gap-2">
              <label htmlFor={nameId} className="font-medium">
                New profile name
              </label>
              <input
                id={nameId}
                value={name}
                aria-invalid={nameError ? true : undefined}
                aria-describedby={nameError ? `${nameId}-error` : undefined}
                onChange={(event) => {
                  setName(event.target.value);
                  setNameError(null);
                }}
                className="px-2 py-1 rounded-md border border-[var(--border)] bg-[var(--bg-surface)]"
              />
            </div>
            {nameError && (
              <p
                id={`${nameId}-error`}
                role="alert"
                className="text-[var(--danger)]"
              >
                {nameError}. Choose another name.
              </p>
            )}
          </div>
          <div className="flex gap-2">
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
            <Button
              size="sm"
              variant="primary"
              disabled={
                busy ||
                !name.trim() ||
                (preview.missing_packages.length > 0 && !acceptIncomplete)
              }
              isLoading={busy}
              onClick={importBundle}
            >
              Create profile and install
            </Button>
          </div>
          {importing && (
            <p role="status" className="text-[var(--fg-muted)]">
              {progress && progress.total > 0
                ? `Installing ${Math.min(progress.done + 1, progress.total)} of ${progress.total}: ${progress.current}`
                : "Reading the bundle and checking each package..."}
            </p>
          )}
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
          {result.settings_applied.length > 0 && (
            <p>
              Settings applied for:{" "}
              {result.settings_applied
                .map((id) => importedNames.get(id.toLowerCase()) ?? id)
                .join(", ")}
              .
            </p>
          )}
          {result.installed_with_package.length > 0 && (
            <p>
              Optional, but installed because each comes in one package with a
              mod that was installed: {result.installed_with_package.join(", ")}
              .
            </p>
          )}
          {result.declined_optional.length > 0 && (
            <p>Optional, left out: {result.declined_optional.join(", ")}.</p>
          )}
          {result.not_for_this_computer.length > 0 && (
            <p>
              Not installed, because they are not for this computer:{" "}
              {result.not_for_this_computer.join(", ")}.
            </p>
          )}
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
          {result.reference_attached && (
            <p className="text-[var(--fg-muted)]">
              The bundle's mod list is kept as the new profile's group
              reference, so anything not installed stays listed there until it
              is resolved.
            </p>
          )}
          <p className="text-[var(--fg-muted)]">
            Switch to the new profile from the list above when you are ready.
          </p>
        </div>
      )}
    </Card>
  );
};
