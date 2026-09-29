import React from "react";
import {
  AlertTriangle,
  CheckCircle2,
  Circle,
  Info,
  XCircle,
} from "lucide-react";

export type BadgeVariant =
  | "success"
  | "warning"
  | "danger"
  | "info"
  | "neutral";

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
    success:
      "bg-[var(--success-surface)] text-[var(--success)] border border-[var(--success)]/20",
    warning:
      "bg-[var(--warning-surface)] text-[var(--warning)] border border-[var(--warning)]/20",
    danger:
      "bg-[var(--danger-surface)] text-[var(--danger)] border border-[var(--danger)]/20",
    info: "bg-[var(--bg-elevated)] text-[var(--accent-primary)] border border-[var(--accent-primary)]/20",
    neutral:
      "bg-[var(--bg-elevated)] text-[var(--fg-muted)] border border-[var(--border)]",
  };

  // Every variant has its own shape and a spoken label, so the status is never
  // carried by colour alone.
  const marker = {
    success: { Icon: CheckCircle2, label: "Success" },
    warning: { Icon: AlertTriangle, label: "Warning" },
    danger: { Icon: XCircle, label: "Problem" },
    info: { Icon: Info, label: "Information" },
    neutral: { Icon: Circle, label: "Status" },
  }[variant];

  return (
    <span
      className={`inline-flex items-center px-2.5 py-1 rounded-full text-xs font-semibold tracking-wide ${variantStyles[variant]} ${className}`}
    >
      <marker.Icon className="w-3 h-3 mr-1.5 shrink-0" aria-hidden="true" />
      <span className="sr-only">{marker.label}: </span>
      {children}
    </span>
  );
};
