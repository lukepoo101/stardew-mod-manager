import React, { useState } from "react";
import { Card } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import { useActiveProfileOverview } from "@/shared/api/hooks";
import type {
  AdoptionResultDto,
  AdoptionScanDto,
} from "@/shared/api/generated";
import { FolderInput } from "lucide-react";

const kb = (bytes: number) => `${Math.max(1, Math.round(bytes / 1024))} KB`;
const STORED: Record<string, string> = {
  exact: "matches a stored package exactly",
  same_version: "this version is stored, but files here differ",
  other_version: "another version is stored",
  none: "",
};

/**
 * Adopts mods from the game's own Mods folder into a new profile. Scanning
 * only reads; adopting copies the chosen folders through the normal install
 * checks, and the original folder is never changed or emptied.
 */
export const AdoptionCard: React.FC = () => {
  const { data: overview } = useActiveProfileOverview();
  const gameId = overview?.game.id;
  const [scan, setScan] = useState<AdoptionScanDto | null>(null);
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const [name, setName] = useState("Adopted mods");
  const [result, setResult] = useState<AdoptionResultDto | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [exported, setExported] = useState<string | null>(null);
  const profileId = overview?.profile.id;
  if (!gameId) return null;

  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await action();
    } catch (actionError) {
      setError(errorSummary(actionError, "Nothing was changed"));
    } finally {
      setBusy(false);
    }
  };

  const doScan = () =>
    run(async () => {
      const found = await api.scanModsFolder(gameId);
      setScan(found);
      setResult(null);
      // Mods with another copy are left for the user to choose between.
      setChosen(
        new Set(
          found.mods
            .filter((m) => m.duplicate_of.length === 0)
            .map((m) => m.folder),
        ),
      );
    });

  const conflicts = (scan?.mods ?? []).filter(
    (m) => chosen.has(m.folder) && m.duplicate_of.some((d) => chosen.has(d)),
  );
  const chosenBytes = (scan?.mods ?? [])
    .filter((m) => chosen.has(m.folder))
    .reduce((sum, m) => sum + m.size_bytes, 0);

  return (
    <Card className="space-y-3 text-xs">
      <div className="flex items-center gap-2 border-b border-[var(--border)] pb-3">
        <FolderInput className="w-4 h-4 text-[var(--accent-primary)]" />
        <h3 className="font-bold text-sm">
          Adopt mods already in the game folder
        </h3>
      </div>
      <p className="text-[var(--fg-muted)]">
        Reads the game's own Mods folder and copies the mods you choose into a
        new profile. Nothing in that folder is moved, changed or deleted, so the
        game still works without the manager.
      </p>
      <Button size="sm" variant="secondary" disabled={busy} onClick={doScan}>
        {scan ? "Scan again" : "Scan the Mods folder"}
      </Button>
      {scan && (
        <div className="space-y-2">
          <p className="font-mono break-all">{scan.mods_dir}</p>
          {scan.manager_markers.length > 0 && (
            <p role="alert" className="text-[var(--warning)]">
              Another mod manager seems to manage these files (
              {scan.manager_markers.join(", ")}). Adopting copies them; that
              manager may still change the originals.
            </p>
          )}
          {scan.mods.length === 0 ? (
            <p>No mods were found to adopt.</p>
          ) : (
            <ul className="space-y-1">
              {scan.mods.map((mod) => (
                <li key={mod.folder}>
                  <label className="flex items-start gap-2">
                    <input
                      type="checkbox"
                      checked={chosen.has(mod.folder)}
                      onChange={(event) => {
                        const next = new Set(chosen);
                        if (event.target.checked) next.add(mod.folder);
                        else next.delete(mod.folder);
                        setChosen(next);
                      }}
                    />
                    <span>
                      <span className="font-medium">{mod.folder}</span>:{" "}
                      {mod.components
                        .map((c) => `${c.name} ${c.version} (${c.unique_id})`)
                        .join(", ")}
                      <span className="text-[var(--fg-muted)]">
                        {" "}
                        · {mod.file_count} file(s), {kb(mod.size_bytes)}
                        {mod.has_settings ? ", with its settings" : ""}
                        {STORED[mod.stored] ? `, ${STORED[mod.stored]}` : ""}
                      </span>
                      {mod.locally_modified.length > 0 && (
                        <span className="block">
                          Changed here compared with the stored package:{" "}
                          {mod.locally_modified.join(", ")}. The adopted copy
                          keeps these changes.
                        </span>
                      )}
                      {mod.duplicate_of.length > 0 && (
                        <span className="block text-[var(--warning)]">
                          The same mod is also in {mod.duplicate_of.join(", ")};
                          adopt only one.
                        </span>
                      )}
                      {mod.problems.map((p) => (
                        <span key={p} className="block text-[var(--warning)]">
                          {p}
                        </span>
                      ))}
                    </span>
                  </label>
                </li>
              ))}
            </ul>
          )}
          {scan.unknown.length > 0 && (
            <details>
              <summary className="cursor-pointer">
                Left where they are ({scan.unknown.length})
              </summary>
              <ul className="list-disc pl-4">
                {scan.unknown.map((u) => (
                  <li key={u.name}>
                    {u.name} ({kb(u.size_bytes)}): {u.reason}
                  </li>
                ))}
              </ul>
            </details>
          )}
          {scan.runtime.length > 0 && (
            <p className="text-[var(--fg-muted)]">
              Part of SMAPI, not adopted: {scan.runtime.join(", ")}.
            </p>
          )}
          {scan.mods.length > 0 && (
            <form
              className="flex flex-wrap gap-2 items-end"
              onSubmit={(event) => {
                event.preventDefault();
                void run(async () => {
                  setResult(
                    await api.adoptMods(
                      gameId,
                      name,
                      [...chosen],
                      scan.fingerprint,
                    ),
                  );
                  setScan(null);
                });
              }}
            >
              <label className="space-y-1">
                <span className="block">New profile name</span>
                <input
                  className="px-2 py-1 rounded-md border border-[var(--border)] bg-[var(--bg-surface)]"
                  value={name}
                  onChange={(event) => setName(event.target.value)}
                />
              </label>
              <Button
                size="sm"
                type="submit"
                disabled={
                  busy ||
                  chosen.size === 0 ||
                  conflicts.length > 0 ||
                  !name.trim()
                }
              >
                Adopt {chosen.size} into a new profile ({kb(chosenBytes)})
              </Button>
            </form>
          )}
          {conflicts.length > 0 && (
            <p role="alert" className="text-[var(--warning)]">
              Two chosen folders hold the same mod; untick one.
            </p>
          )}
        </div>
      )}
      {result && (
        <div role="status" className="space-y-1">
          <p>
            Made "{result.profile_name}" with {result.adopted.length} adopted
            folder(s). The Mods folder was not changed.
          </p>
          {result.failed.map((f) => (
            <p key={f.folder} className="text-[var(--warning)]">
              {f.folder} was not adopted: {f.reason}
            </p>
          ))}
          {result.left_in_place.length > 0 && (
            <p className="text-[var(--fg-muted)]">
              Left as they were: {result.left_in_place.join(", ")}.
            </p>
          )}
        </div>
      )}
      <div className="border-t border-[var(--border)] pt-2 space-y-1">
        <p className="text-[var(--fg-muted)]">
          Leaving the manager? Copy this profile's enabled mods into a plain
          Mods folder that SMAPI can use on its own. Nothing in the profile
          changes.
        </p>
        <Button
          size="sm"
          variant="secondary"
          disabled={busy || !profileId}
          onClick={() =>
            run(async () => {
              const dir = await api.pickFolderDialog();
              if (!dir || !profileId) return;
              setExported(await api.exportModsFolder(profileId, dir));
            })
          }
        >
          Copy this profile's mods to a folder…
        </Button>
        {exported && (
          <p role="status">
            Copied to {exported}. Copy its contents into the game's Mods folder,
            or start SMAPI with --mods-path pointing at it.
          </p>
        )}
      </div>
      {error && (
        <p role="alert" className="text-[var(--danger)]">
          {error}
        </p>
      )}
    </Card>
  );
};
