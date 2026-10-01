import React, { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Card } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { CopyButton } from "@/components/ui/CopyButton";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import { FolderTree } from "lucide-react";

/**
 * Where the game, its saves and the manager's own data live, with what each
 * holds and whether it is safe to clear. Folders are opened by id; the page
 * never asks to open an arbitrary path.
 */
export const LocationsCard: React.FC = () => {
  const { data: locations } = useQuery({
    queryKey: ["locations"],
    queryFn: () => api.getLocations(),
  });
  const [error, setError] = useState<string | null>(null);

  return (
    <Card className="space-y-3">
      <div className="flex items-center gap-2 border-b border-[var(--border)] pb-3">
        <FolderTree className="w-4 h-4 text-[var(--accent-primary)]" />
        <h3 className="font-bold text-sm">Where things are</h3>
      </div>
      <details className="text-xs">
        <summary className="cursor-pointer text-[var(--fg-muted)]">
          Show the folders the game and the manager use
        </summary>
        <ul className="mt-2 space-y-2">
          {(locations ?? []).map((location) => (
            <li key={location.id} className="space-y-0.5">
              <div className="flex flex-wrap items-center gap-2">
                <span className="font-semibold">{location.label}</span>
                {location.path ? (
                  <>
                    <span className="font-mono break-all">{location.path}</span>
                    <CopyButton value={location.path} label={location.label} />
                    {location.exists ? (
                      <Button
                        size="sm"
                        variant="ghost"
                        onClick={async () => {
                          setError(null);
                          try {
                            await api.revealLocation(location.id);
                          } catch (openError) {
                            setError(
                              errorSummary(openError, "Could not open it"),
                            );
                          }
                        }}
                      >
                        Open
                      </Button>
                    ) : (
                      <span className="text-[var(--fg-muted)]">
                        (not there yet)
                      </span>
                    )}
                  </>
                ) : (
                  <span className="text-[var(--fg-muted)]">not set up</span>
                )}
              </div>
              <p className="text-[var(--fg-muted)]">{location.note}</p>
            </li>
          ))}
        </ul>
      </details>
      {error && (
        <p role="alert" className="text-xs text-[var(--danger)]">
          {error}
        </p>
      )}
    </Card>
  );
};
