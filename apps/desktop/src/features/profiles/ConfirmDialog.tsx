import React, { useCallback, useId, useRef, useState } from "react";
import { Modal } from "@/components/ui/Modal";
import { Button } from "@/components/ui/Button";

export interface Question {
  title: string;
  /** What will happen, in plain words. Line breaks are kept. */
  message?: React.ReactNode;
  /** The button that goes ahead, such as "Publish" or "Accept". */
  confirmLabel: string;
  /** An optional text field, such as the reason for accepting something. */
  field?: { label: string; initial?: string; placeholder?: string };
}

interface Open extends Question {
  resolve: (answer: string | null) => void;
}

/**
 * Asks before something happens, in the app's own dialog rather than the
 * browser's: keyboard focus stays inside it, Escape cancels, and Cancel
 * comes first. `ask` resolves to the field's text (empty when there is no
 * field) when the user goes ahead, or `null` when they cancel.
 */
export function useConfirm(): [
  React.ReactNode,
  (question: Question) => Promise<string | null>,
] {
  const [open, setOpen] = useState<Open | null>(null);
  const [text, setText] = useState("");
  const titleId = useId();
  const fieldId = useId();
  const pending = useRef<Open | null>(null);

  const ask = useCallback(
    (question: Question) =>
      new Promise<string | null>((resolve) => {
        // A question still open is answered "no" before the next one.
        pending.current?.resolve(null);
        const next = { ...question, resolve };
        pending.current = next;
        setText(question.field?.initial ?? "");
        setOpen(next);
      }),
    [],
  );

  const answer = (value: string | null) => {
    open?.resolve(value);
    pending.current = null;
    setOpen(null);
  };

  const dialog = open ? (
    <Modal
      labelledBy={titleId}
      onClose={() => answer(null)}
      className="space-y-3 text-sm max-h-[85vh] overflow-y-auto"
    >
      <h2 id={titleId} className="text-lg font-bold">
        {open.title}
      </h2>
      {open.message && (
        <div className="whitespace-pre-line text-[var(--fg-muted)]">
          {open.message}
        </div>
      )}
      <form
        className="space-y-3"
        onSubmit={(event) => {
          event.preventDefault();
          answer(open.field ? text : "");
        }}
      >
        {open.field && (
          <div className="space-y-1">
            <label htmlFor={fieldId} className="block font-medium">
              {open.field.label}
            </label>
            <input
              id={fieldId}
              autoFocus
              value={text}
              placeholder={open.field.placeholder}
              maxLength={300}
              onChange={(event) => setText(event.target.value)}
              className="w-full px-2 py-1 rounded-md border border-[var(--border)] bg-[var(--bg-surface)]"
            />
          </div>
        )}
        <div className="flex justify-end gap-2">
          <Button
            type="button"
            variant="secondary"
            onClick={() => answer(null)}
          >
            Cancel
          </Button>
          <Button type="submit">{open.confirmLabel}</Button>
        </div>
      </form>
    </Modal>
  ) : null;

  return [dialog, ask];
}
