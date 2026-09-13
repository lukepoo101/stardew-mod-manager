import React, { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { ArchiveInspectionResult } from "@/lib/backend/types";
import { backend } from "@/lib/backend/client";

export interface ModDropZoneProps {
  setupId: string;
  onInspectionReady: (result: ArchiveInspectionResult) => void;
}

export const ModDropZone: React.FC<ModDropZoneProps> = ({
  setupId,
  onInspectionReady,
}) => {
  const [isDragging, setIsDragging] = useState(false);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [manualPath, setManualPath] = useState("");
  const fileInputRef = useRef<HTMLInputElement>(null);

  const handleFile = async (filePath: string) => {
    if (!filePath.trim()) return;
    setIsLoading(true);
    setError(null);
    try {
      const result = await backend.inspectMod(filePath.trim(), setupId);
      onInspectionReady(result);
    } catch (e: any) {
      setError(e?.toString() || "Failed to inspect mod archive");
    } finally {
      setIsLoading(false);
    }
  };

  // Listen for native OS drag and drop events (provides real filesystem paths)
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    import("@tauri-apps/api/webview")
      .then(({ getCurrentWebview }) => {
        getCurrentWebview()
          .onDragDropEvent((event) => {
            if (event.payload.type === "drop" && event.payload.paths.length > 0) {
              const dropped = event.payload.paths[0];
              if (dropped.toLowerCase().endsWith(".zip")) {
                handleFile(dropped);
              }
            }
          })
          .then((fn) => {
            unlisten = fn;
          });
      })
      .catch(() => {
        // Outside Tauri runtime
      });

    return () => {
      if (unlisten) unlisten();
    };
  }, [setupId]);

  const handleChoose = async (e?: React.MouseEvent) => {
    e?.stopPropagation();
    try {
      const picked = await backend.pickModFile();
      if (picked) {
        handleFile(picked);
        return;
      }
    } catch {
      // Fallback
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
    if (e.dataTransfer.files && e.dataTransfer.files.length > 0) {
      const file = e.dataTransfer.files[0];
      const filePath = (file as any).path || file.name;
      handleFile(filePath);
    }
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
