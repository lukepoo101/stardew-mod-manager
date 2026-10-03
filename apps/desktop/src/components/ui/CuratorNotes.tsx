import React from "react";
import { CopyButton } from "@/components/ui/CopyButton";
import { linksIn } from "@/shared/recipe/notes";

/**
 * A curator's notes, shown as their words: plain text, with any web links
 * offered for copying rather than opened.
 */
export const CuratorNotes: React.FC<{ notes: string; className?: string }> = ({
  notes,
  className,
}) => {
  if (!notes.trim()) return null;
  return (
    <div className={className}>
      <p className="whitespace-pre-wrap">{notes}</p>
      {linksIn(notes).map((link) => (
        <span
          key={link}
          className="flex items-center gap-1 font-mono break-all"
        >
          {link}
          <CopyButton value={link} label={`the link ${link}`} />
        </span>
      ))}
    </div>
  );
};
