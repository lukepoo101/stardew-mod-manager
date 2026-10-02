import React, { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { api } from "@/shared/api/client";
import { formatBytes } from "@/features/settings/StorageCleanupCard";

const PAGE = 200;

/**
 * What the install of a mod put in its folder, from the manager's records,
 * plus the package it came from: the other mods in it and whether the stored
 * copy is still intact. Nothing here runs or opens a file.
 */
export const PackageFiles: React.FC<{ profileComponentId: string }> = ({
  profileComponentId,
}) => {
  const { data } = useQuery({
    queryKey: ["mod-package-files", profileComponentId],
    queryFn: () => api.getModPackageFiles(profileComponentId),
    staleTime: 5000,
  });
  const [query, setQuery] = useState("");
  const [shown, setShown] = useState(PAGE);
  const matching = useMemo(() => {
    const q = query.trim().toLowerCase();
    return (data?.files ?? []).filter(
      (f) => !q || f.path.toLowerCase().includes(q),
    );
  }, [data, query]);
  if (!data) return null;

  return (
    <div className="space-y-1 text-xs">
      <h4 className="font-bold text-[var(--fg-muted)] uppercase tracking-wider">
        Package and files
      </h4>
      <p>
        Package {data.artifact_hash.slice(0, 12)}…{" "}
        {data.package_stored
          ? data.package_intact
            ? "is stored and matches its checksum."
            : "is stored but no longer matches its checksum."
          : "is no longer stored."}
      </p>
      {data.package_mods.length > 1 && (
        <p>Mods from this package: {data.package_mods.join(", ")}.</p>
      )}
      {!data.recorded ? (
        <p className="text-[var(--fg-muted)]">
          Installed before the manager recorded files, so there is no file list.
        </p>
      ) : (
        <details>
          <summary className="cursor-pointer">
            Installed files ({data.files.length})
          </summary>
          <label className="flex items-center gap-2 my-1">
            <span className="text-[var(--fg-muted)]">Find</span>
            <input
              type="search"
              value={query}
              onChange={(event) => {
                setQuery(event.target.value);
                setShown(PAGE);
              }}
              className="px-2 py-1 rounded-md border border-[var(--border)] bg-[var(--bg-surface)]"
            />
          </label>
          <ul className="font-mono break-all">
            {matching.slice(0, shown).map((file) => (
              <li key={file.path}>
                {file.path}{" "}
                <span className="text-[var(--fg-muted)]">
                  {formatBytes(file.size_bytes)}
                </span>
              </li>
            ))}
          </ul>
          {matching.length > shown && (
            <button
              type="button"
              className="underline cursor-pointer"
              onClick={() => setShown((n) => n + PAGE)}
            >
              Show more ({matching.length - shown} left)
            </button>
          )}
          <p className="text-[var(--fg-muted)]">
            This is what was installed. Diagnostics → Check mod files compares
            it with what is in the folder now.
          </p>
        </details>
      )}
    </div>
  );
};
