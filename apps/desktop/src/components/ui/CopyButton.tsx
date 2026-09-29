import React, { useState } from "react";
import { Copy } from "lucide-react";
import { copyText } from "@/shared/support/actions";

/** Copies exactly `value` and announces the result to assistive technology. */
export const CopyButton: React.FC<{ value: string; label: string }> = ({
  value,
  label,
}) => {
  const [status, setStatus] = useState<"idle" | "copied" | "failed">("idle");
  return (
    <>
      <button
        type="button"
        onClick={async () => {
          setStatus((await copyText(value)) ? "copied" : "failed");
          setTimeout(() => setStatus("idle"), 2500);
        }}
        aria-label={`Copy ${label}`}
        title={`Copy ${label}`}
        className="p-1 rounded hover:bg-[var(--bg-elevated)] text-[var(--fg-muted)] cursor-pointer"
      >
        <Copy className="w-3.5 h-3.5" aria-hidden="true" />
      </button>
      <span role="status" className="sr-only">
        {status === "copied"
          ? `${label} copied`
          : status === "failed"
            ? "Could not access the clipboard"
            : ""}
      </span>
    </>
  );
};
