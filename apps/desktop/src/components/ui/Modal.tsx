import React, { useState } from "react";
import * as RadixDialog from "@radix-ui/react-dialog";
import { focusedElement, returnFocus } from "@/shared/a11y/focusReturn";

export interface ModalProps {
  /** Id of the element inside that names the dialog, usually its heading. */
  labelledBy: string;
  /** Called on Escape or an outside click. Omit while something is running
   * so the dialog cannot be dismissed half way. */
  onClose?: () => void;
  className?: string;
  /** Where focus goes when it closes, when that is not the control focused
   * as it opened (one that was busy and disabled while it was opening has
   * lost focus by then). Asked at close, so it can find a row that moved. */
  returnFocusTo?: () => HTMLElement | null;
  children: React.ReactNode;
}

/**
 * A modal for dialogs that lay out their own heading and buttons. Radix keeps
 * keyboard focus inside it and closes it on Escape. When it closes, focus
 * goes back to the control that was focused when it opened.
 */
export const Modal: React.FC<ModalProps> = ({
  labelledBy,
  onClose,
  className = "",
  returnFocusTo,
  children,
}) => {
  // Read during the first render, before focus moves into the dialog.
  const [opener] = useState(focusedElement);
  return (
    <RadixDialog.Root
      open
      onOpenChange={(open) => {
        if (!open) onClose?.();
      }}
    >
      <RadixDialog.Portal>
        <RadixDialog.Overlay className="fixed inset-0 z-50 bg-black/50" />
        <div className="fixed inset-0 z-50 flex items-center justify-center p-6 pointer-events-none">
          <RadixDialog.Content
            aria-labelledby={labelledBy}
            aria-describedby={undefined}
            onEscapeKeyDown={(event) => {
              if (!onClose) event.preventDefault();
            }}
            onPointerDownOutside={(event) => {
              if (!onClose) event.preventDefault();
            }}
            onCloseAutoFocus={(event) =>
              returnFocus(returnFocusTo?.() ?? opener, event)
            }
            className={`pointer-events-auto bg-[var(--bg-surface)] border border-[var(--border)] rounded-xl p-6 w-full max-w-lg max-h-full overflow-y-auto focus:outline-none ${className}`}
          >
            {children}
          </RadixDialog.Content>
        </div>
      </RadixDialog.Portal>
    </RadixDialog.Root>
  );
};
