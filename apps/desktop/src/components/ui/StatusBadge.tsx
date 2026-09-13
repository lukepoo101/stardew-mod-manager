import React from "react";

export type BadgeVariant = "success" | "warning" | "danger" | "info" | "neutral";

export interface StatusBadgeProps {
  variant?: BadgeVariant;
  children: React.ReactNode;
  className?: string;
}

export const StatusBadge: React.FC<StatusBadgeProps> = ({
  variant = "neutral",
  children,
  className = "",
}) => {
  const variantStyles = {
    success: "bg-[var(--success-surface)] text-[var(--success)] border border-[var(--success)]/20",
    warning: "bg-[var(--warning-surface)] text-[var(--warning)] border border-[var(--warning)]/20",
    danger: "bg-[var(--danger-surface)] text-[var(--danger)] border border-[var(--danger)]/20",
    info: "bg-[var(--bg-elevated)] text-[var(--accent-primary)] border border-[var(--accent-primary)]/20",
    neutral: "bg-[var(--bg-elevated)] text-[var(--fg-muted)] border border-[var(--border)]",
  };

  return (
    <span
      className={`inline-flex items-center px-2.5 py-1 rounded-full text-xs font-semibold tracking-wide ${variantStyles[variant]} ${className}`}
    >
      <span className="w-1.5 h-1.5 rounded-full bg-current mr-1.5 opacity-80" />
      {children}
    </span>
  );
};
