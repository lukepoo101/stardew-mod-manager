import React from "react";
import { type RedactionResult, residualWarnings } from "./redact";

/** The review warnings for a redacted text, as the support export lists them. */
export function redactionWarnings(
  result: RedactionResult,
  names: readonly string[],
): string[] {
  return [...result.review, ...residualWarnings(result.text, names)];
}

/**
 * Shows what still needs a human eye in a redacted text before it is
 * copied, the same warnings the support export shows.
 */
export const RedactionWarnings: React.FC<{ warnings: readonly string[] }> = ({
  warnings,
}) =>
  warnings.length === 0 ? null : (
    <ul
      className="text-xs space-y-1 p-3 rounded-lg border border-[var(--warning)]/30 bg-[var(--warning-surface)] text-[var(--warning)]"
      aria-label="Review before sharing"
    >
      {warnings.map((warning) => (
        <li key={warning}>{warning}</li>
      ))}
    </ul>
  );
