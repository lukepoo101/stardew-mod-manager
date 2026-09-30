import React from "react";
import * as RadixDialog from "@radix-ui/react-dialog";

export interface ModalProps {
  /** Id of the element inside that names the dialog, usually its heading. */
  labelledBy: string;
  /** Called on Escape or an outside click. Omit while something is running
   * so the dialog cannot be dismissed half way. */
  onClose?: () => void;
  className?: string;
  children: React.ReactNode;
}

/**
 * A modal for dialogs that lay out their own heading and buttons. Radix keeps
 * keyboard focus inside it, closes it on Escape and returns focus to the
 * control that opened it.
 */
export const Modal: React.FC<ModalProps> = ({
  labelledBy,
  onClose,
  className = "",
  children,
}) => (
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
          className={`pointer-events-auto bg-[var(--bg-surface)] border border-[var(--border)] rounded-xl p-6 w-full max-w-lg focus:outline-none ${className}`}
        >
          {children}
        </RadixDialog.Content>
      </div>
    </RadixDialog.Portal>
  </RadixDialog.Root>
);
