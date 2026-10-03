import React from "react";
import { CopyButton } from "@/components/ui/CopyButton";
import type { RecipeComponent } from "@/shared/recipe/recipe";

/**
 * Who made a mod and where it is published, as the recipe records it: the
 * mod's author, the update keys its manifest declares, and any page the
 * sharer added. Missing information is said to be missing, never guessed.
 */
export const Provenance: React.FC<{ component: RecipeComponent }> = ({
  component,
}) => {
  const keys = component.update_keys ?? [];
  return (
    <span className="block text-[var(--fg-muted)]">
      By {component.author.trim() || "an unknown author"}.{" "}
      {keys.length > 0
        ? `Published at ${keys.join(", ")} (as its manifest says).`
        : "Where it is published is not recorded."}
      {component.source_url && (
        <span className="inline-flex items-center gap-1">
          {" "}
          Page added by the sharer:{" "}
          <span className="font-mono break-all">{component.source_url}</span>
          <CopyButton value={component.source_url} label="the source page" />
        </span>
      )}
    </span>
  );
};
