import React from "react";
import * as RadixDialog from "@radix-ui/react-dialog";

export interface DialogProps {
  isOpen: boolean;
  onClose: () => void;
  title: string;
  children: React.ReactNode;
  footer?: React.ReactNode;
}

export const Dialog: React.FC<DialogProps> = ({
  isOpen,
  onClose,
  title,
  children,
  footer,
}) => {
  return (
    <RadixDialog.Root open={isOpen} onOpenChange={(open) => { if (!open) onClose(); }}>
      <RadixDialog.Portal>
        <RadixDialog.Overlay className="fixed inset-0 z-50 bg-black/50 backdrop-blur-xs animate-in fade-in duration-150" />
        <div className="fixed inset-0 z-50 flex items-center justify-center p-4 pointer-events-none">
          <RadixDialog.Content
            className="pointer-events-auto bg-[var(--bg-surface)] border border-[var(--border)] rounded-2xl w-full max-w-lg shadow-xl overflow-hidden animate-in fade-in zoom-in-95 duration-150 focus:outline-none"
          >
            <div className="flex items-center justify-between p-5 border-b border-[var(--border)]">
              <RadixDialog.Title className="text-lg font-bold text-[var(--fg-primary)]">
                {title}
              </RadixDialog.Title>
              <RadixDialog.Description className="sr-only">
                {title}
              </RadixDialog.Description>
              <RadixDialog.Close asChild>
                <button
                  className="text-[var(--fg-muted)] hover:text-[var(--fg-primary)] min-h-[40px] min-w-[40px] p-1 rounded-md text-xl leading-none"
                  aria-label="Close dialog"
                >
                  ×
                </button>
              </RadixDialog.Close>
            </div>

            <div className="p-6 text-[var(--fg-primary)] space-y-4 max-h-[70vh] overflow-y-auto">
              {children}
            </div>

            {footer && (
              <div className="flex items-center justify-end gap-3 p-4 bg-[var(--bg-elevated)] border-t border-[var(--border)]">
                {footer}
              </div>
            )}
          </RadixDialog.Content>
        </div>
      </RadixDialog.Portal>
    </RadixDialog.Root>
  );
};
