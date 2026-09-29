import React from "react";
import { Card } from "@/components/ui/Card";
import { usePreferences } from "@/shared/preferences";

export interface EmptyStateProps {
  icon?: React.ReactNode;
  title: string;
  /** Why it is empty and what the action does. Hidden when the user turns
   * guidance text off; the actions always stay. */
  description?: React.ReactNode;
  /** The next safe step, first in reading and tab order after the title. */
  actions?: React.ReactNode;
  className?: string;
}

/**
 * A zero state that says why a view is empty and what to do next. Copy and
 * actions belong to the feature that renders it, so the wording stays exact;
 * this only fixes the layout and the guidance preference.
 */
export const EmptyState: React.FC<EmptyStateProps> = ({
  icon,
  title,
  description,
  actions,
  className = "",
}) => {
  const [preferences] = usePreferences();
  return (
    <Card className={`text-center py-10 space-y-3 ${className}`}>
      {icon && (
        <div
          aria-hidden="true"
          className="w-12 h-12 rounded-full bg-[var(--bg-elevated)] text-[var(--fg-muted)] flex items-center justify-center mx-auto"
        >
          {icon}
        </div>
      )}
      <p className="text-sm font-semibold text-[var(--fg-primary)]">{title}</p>
      {preferences.showGuidance && description && (
        <p className="text-xs text-[var(--fg-muted)] max-w-md mx-auto leading-relaxed">
          {description}
        </p>
      )}
      {actions && (
        <div className="flex flex-wrap items-center justify-center gap-2 pt-1">
          {actions}
        </div>
      )}
    </Card>
  );
};

/** A load failure, shown instead of an empty state so the two are never confused. */
export const LoadFailed: React.FC<{
  what: string;
  message: string;
  onRetry?: () => void;
}> = ({ what, message, onRetry }) => (
  <Card className="py-6 space-y-2 text-center border border-[var(--danger)]/30">
    <p role="alert" className="text-sm font-semibold text-[var(--danger)]">
      Could not load {what}
    </p>
    <p className="text-xs text-[var(--fg-muted)]">{message}</p>
    {onRetry && (
      <button
        type="button"
        onClick={onRetry}
        className="text-xs font-medium text-[var(--accent-primary)] hover:underline cursor-pointer"
      >
        Try again
      </button>
    )}
  </Card>
);

/** A neutral loading placeholder that never claims the view is empty. */
export const Loading: React.FC<{ what: string }> = ({ what }) => (
  <Card className="text-center py-10" role="status">
    <div className="w-6 h-6 border-2 border-[var(--accent-primary)] border-t-transparent rounded-full animate-spin mx-auto mb-2" />
    <p className="text-xs text-[var(--fg-muted)]">Loading {what}...</p>
  </Card>
);
