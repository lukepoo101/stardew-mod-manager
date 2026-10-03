import React, { useState } from "react";
import type { DependencyMapEntryDto } from "@/shared/api/generated";
import {
  brokenWithout,
  cycles,
  edges,
  neighbourhood,
} from "@/shared/recipe/graph";
import type { ProfileRecipe } from "@/shared/recipe/recipe";

const KIND: Record<string, string> = {
  required: "needs",
  optional: "can use",
  content_pack_for: "is a content pack for",
};
const STATUS: Record<string, string> = {
  satisfied: "",
  missing: " (missing)",
  disabled: " (disabled)",
  too_old: " (too old)",
};

/** The collection's dependency graph as a filterable, focusable list. */
export const CollectionGraph: React.FC<{
  recipe: ProfileRecipe;
  map: readonly DependencyMapEntryDto[];
}> = ({ recipe, map }) => {
  const [filter, setFilter] = useState("");
  const [focus, setFocus] = useState("");
  const [declineOptional, setDeclineOptional] = useState(false);
  const all = edges(map);
  const loops = cycles(map);
  const near = focus ? neighbourhood(map, focus) : null;
  const optional = new Set(
    recipe.components
      .filter((c) => c.optional)
      .map((c) => c.unique_id.toLowerCase()),
  );
  const broken = declineOptional ? brokenWithout(map, optional) : [];
  const shown = map.filter(
    (m) =>
      (!near || near.has(m.unique_id.toLowerCase())) &&
      (!filter ||
        m.name.toLowerCase().includes(filter.toLowerCase()) ||
        m.unique_id.toLowerCase().includes(filter.toLowerCase())),
  );
  const input =
    "px-2 py-1 rounded-md border border-[var(--border)] bg-[var(--bg-surface)]";
  return (
    <details>
      <summary className="cursor-pointer font-semibold">
        Dependency graph ({map.length} mods, {all.length} links)
      </summary>
      <div className="space-y-2 mt-1">
        {loops.length > 0 && (
          <p role="alert" className="text-[var(--danger)]">
            Requirement loops: {loops.map((l) => l.join(" → ")).join("; ")}
          </p>
        )}
        <div className="flex flex-wrap gap-2 items-center">
          <input
            aria-label="Filter the graph"
            className={input}
            placeholder="Filter by name or UniqueID"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
          />
          <select
            aria-label="Focus on a mod"
            className={input}
            value={focus}
            onChange={(e) => setFocus(e.target.value)}
          >
            <option value="">All mods</option>
            {map.map((m) => (
              <option key={m.unique_id} value={m.unique_id}>
                {m.name}
              </option>
            ))}
          </select>
          {optional.size > 0 && (
            <label className="flex items-center gap-1">
              <input
                type="checkbox"
                checked={declineOptional}
                onChange={(e) => setDeclineOptional(e.target.checked)}
              />
              Show what breaks if a recipient declines every optional mod
            </label>
          )}
        </div>
        {declineOptional && (
          <p>
            {broken.length > 0
              ? `These would stop loading: ${broken.join(", ")}.`
              : "Nothing else depends on the optional mods."}
          </p>
        )}
        <ul className="space-y-1">
          {shown.map((m) => {
            const out = all.filter((e) => e.from === m.unique_id);
            return (
              <li key={m.unique_id}>
                <span className="font-medium">{m.name}</span>
                {optional.has(m.unique_id.toLowerCase()) && (
                  <span className="text-[var(--fg-muted)]"> (optional)</span>
                )}
                {broken.includes(m.name) && (
                  <span className="text-[var(--warning)]"> (would break)</span>
                )}
                {out.length > 0 && (
                  <ul className="pl-4">
                    {out.map((e) => (
                      <li
                        key={`${e.kind}:${e.to}`}
                        className={
                          e.status === "satisfied"
                            ? undefined
                            : "text-[var(--danger)]"
                        }
                      >
                        {KIND[e.kind] ?? e.kind} {e.toName}
                        {STATUS[e.status] ?? ` (${e.status})`}
                      </li>
                    ))}
                  </ul>
                )}
              </li>
            );
          })}
        </ul>
      </div>
    </details>
  );
};
