import React from "react";
import {
  bulkProgress,
  summarise,
  useBulkProgress,
} from "@/shared/progress/bulk";

/**
 * Where a bulk job has got to, in the sidebar, so it stays visible on every
 * page. Each item's state is listed on request; the summary stays after it
 * finishes until dismissed, and the full record is in Activity.
 */
export const BulkProgressIndicator: React.FC = () => {
  const job = useBulkProgress();
  if (!job) return null;
  const { line } = summarise(job.items);
  const running = job.items.find((item) => item.state === "running");
  const handled = job.items.filter(
    (item) => item.state !== "queued" && item.state !== "running",
  ).length;
  return (
    <>
      {/* Narrow sidebar: a compact count, with the summary as its name. */}
      <div
        className="lg:hidden mx-2 text-[10px] text-center font-mono"
        title={`${job.title}: ${line}`}
        role="status"
        aria-label={`${job.title}: ${line}`}
      >
        {handled}/{job.items.length}
      </div>
      <div
        className="hidden lg:block mx-3 p-2 rounded-lg border border-[var(--border)] text-xs space-y-1"
        aria-live="polite"
      >
        <p className="font-semibold">{job.title}</p>
        <p>
          {line}
          {running && !job.finished ? `: now ${running.name}` : ""}
        </p>
        <details>
          <summary className="cursor-pointer text-[var(--fg-muted)]">
            Each item
          </summary>
          <ul className="mt-1 space-y-0.5">
            {job.items.map((item, index) => (
              <li key={index}>
                {item.name}: {item.state}
                {item.message ? ` (${item.message})` : ""}
              </li>
            ))}
          </ul>
        </details>
        {job.finished && (
          <button
            type="button"
            className="underline cursor-pointer text-[var(--fg-muted)]"
            onClick={() => bulkProgress.clear()}
          >
            Dismiss
          </button>
        )}
      </div>
    </>
  );
};
