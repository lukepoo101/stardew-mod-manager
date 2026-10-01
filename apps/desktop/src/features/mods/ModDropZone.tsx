import React, { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";

/**
 * What to do with dropped files: install exactly one .zip, or say why not.
 * Nothing dropped is ever ignored silently.
 */
export function chooseDropped(
  paths: readonly string[],
): { path: string } | { error: string } {
  const name = (path: string) => path.split(/[\\/]/).pop() || path;
  if (paths.length === 0) return { error: "Nothing was dropped." };
  if (paths.length > 1) {
    return {
      error: `Drop one mod archive at a time. ${paths.length} files were dropped (${paths
        .map(name)
        .join(", ")}); none were opened.`,
    };
  }
  const [path] = paths;
  if (!path.toLowerCase().endsWith(".zip")) {
    return {
      error: `${name(path)} is not a .zip mod archive. Extract or download the mod's .zip file and drop that instead.`,
    };
  }
  return { path };
}

export interface ModDropZoneProps {
  onArchiveSelected: (path: string) => Promise<void>;
  /** When given, several dropped .zip files start a batch install. */
  onArchivesSelected?: (paths: string[]) => void;
}

/**
 * Collects a mod archive path from the native picker, OS drag-and-drop, or a
 * pasted path, then hands it to the owning feature. Archive inspection and
 * installation previews belong to the modern API layer, not to this component.
 */
export const ModDropZone: React.FC<ModDropZoneProps> = ({
  onArchiveSelected,
  onArchivesSelected,
}) => {
  const [isDragging, setIsDragging] = useState(false);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [manualPath, setManualPath] = useState("");
  const fileInputRef = useRef<HTMLInputElement>(null);

  // The native drag-and-drop listener subscribes once, so keep the latest
  // callback reachable without re-subscribing on every parent render.
  const archiveSelectedRef = useRef(onArchiveSelected);
  useEffect(() => {
    archiveSelectedRef.current = onArchiveSelected;
  }, [onArchiveSelected]);
  const archivesSelectedRef = useRef(onArchivesSelected);
  useEffect(() => {
    archivesSelectedRef.current = onArchivesSelected;
  }, [onArchivesSelected]);

  /** One .zip opens; several start a batch when one is offered. */
  const route = (paths: string[]) => {
    const batch = archivesSelectedRef.current;
    if (paths.length > 1 && batch) {
      const notZip = paths.filter((p) => !p.toLowerCase().endsWith(".zip"));
      if (notZip.length > 0) {
        setError(
          `Only .zip mod archives can be installed; ${notZip
            .map((p) => p.split(/[\\/]/).pop() || p)
            .join(
              ", ",
            )} ${notZip.length === 1 ? "is" : "are"} not. Nothing was opened.`,
        );
        return;
      }
      setError(null);
      batch(paths);
      return;
    }
    const choice = chooseDropped(paths);
    if ("path" in choice) handleFile(choice.path);
    else setError(choice.error);
  };

  const handleFile = async (filePath: string) => {
    if (!filePath.trim()) return;
    setIsLoading(true);
    setError(null);
    try {
      await archiveSelectedRef.current(filePath.trim());
    } catch (e: unknown) {
      setError(errorSummary(e, "Failed to inspect mod archive"));
    } finally {
      setIsLoading(false);
    }
  };

  // Listen for native OS drag and drop events (provides real filesystem paths)
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    import("@tauri-apps/api/webview")
      .then(({ getCurrentWebview }) => {
        return getCurrentWebview()
          .onDragDropEvent((event) => {
            if (event.payload.type === "drop") route(event.payload.paths);
          })
          .then((fn) => {
            if (disposed) fn();
            else unlisten = fn;
          });
      })
      .catch(() => {
        // Outside Tauri runtime
      });

    return () => {
      disposed = true;
      if (unlisten) unlisten();
    };
  }, []);

  const handleChoose = async (e?: React.MouseEvent) => {
    e?.stopPropagation();
    if (isLoading) return;
    try {
      const picked = await api.pickArchiveDialog();
      if (picked) {
        await handleFile(picked);
        return;
      }
      if ("__TAURI_INTERNALS__" in window) return;
    } catch (error) {
      if ("__TAURI_INTERNALS__" in window) {
        setError(errorSummary(error, "Failed to open the native file picker"));
        return;
      }
    }
    fileInputRef.current?.click();
  };

  const onDragOver = (e: React.DragEvent) => {
    e.preventDefault();
    setIsDragging(true);
  };

  const onDragLeave = () => {
    setIsDragging(false);
  };

  const onDrop = (e: React.DragEvent) => {
    e.preventDefault();
    setIsDragging(false);
    const files = Array.from(e.dataTransfer.files ?? []);
    if (files.length === 0) return;
    route(
      files.map((file) => (file as File & { path?: string }).path || file.name),
    );
  };

  return (
    <div className="space-y-4">
      <Card
        onDragOver={onDragOver}
        onDragLeave={onDragLeave}
        onDrop={onDrop}
        className={`border-2 border-dashed text-center py-10 px-6 transition-colors cursor-pointer ${
          isDragging
            ? "border-[var(--accent-primary)] bg-[var(--accent-primary)]/5"
            : "border-[var(--border)] hover:border-[var(--border-focus)]"
        }`}
        onClick={() => handleChoose()}
      >
        <input
          ref={fileInputRef}
          type="file"
          accept=".zip"
          className="hidden"
          onChange={(e) => {
            if (e.target.files && e.target.files.length > 0) {
              const file = e.target.files[0];
              const filePath = (file as any).path || file.name;
              handleFile(filePath);
            }
          }}
        />

        <div className="space-y-3 max-w-sm mx-auto">
          <div className="w-12 h-12 mx-auto rounded-full bg-[var(--bg-elevated)] flex items-center justify-center text-2xl">
            📦
          </div>
          <div>
            <h3 className="font-bold text-base text-[var(--fg-primary)]">
              Add your mod ZIP
            </h3>
            <p className="text-xs text-[var(--fg-muted)] mt-1">
              Drag and drop a downloaded mod ZIP here, or click to browse.
            </p>
          </div>

          <div className="pt-2">
            <Button
              id="choose-mod-zip"
              variant="secondary"
              size="md"
              isLoading={isLoading}
              onClick={handleChoose}
            >
              Choose mod ZIP
            </Button>
          </div>
        </div>
      </Card>

      {/* Manual file path fallback */}
      <div className="flex items-center gap-2">
        <input
          type="text"
          placeholder="Or paste path or file name (e.g. ~/Downloads/mod.zip)"
          value={manualPath}
          onChange={(e) => setManualPath(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && manualPath.trim()) {
              handleFile(manualPath.trim());
            }
          }}
          className="flex-1 px-3 py-2 text-xs rounded-lg border border-[var(--border)] bg-[var(--bg-surface)] text-[var(--fg-primary)] select-text"
        />
        <Button
          size="md"
          variant="secondary"
          disabled={!manualPath.trim() || isLoading}
          onClick={() => handleFile(manualPath.trim())}
        >
          Inspect
        </Button>
      </div>

      {error && (
        <div className="p-3 bg-[var(--danger-surface)] border border-[var(--danger)]/30 rounded-lg text-xs text-[var(--danger)] leading-relaxed select-text">
          <strong>Inspection error:</strong> {error}
        </div>
      )}
    </div>
  );
};
