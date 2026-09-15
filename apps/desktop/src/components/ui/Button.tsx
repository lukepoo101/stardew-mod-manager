import React from "react";

export interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: "primary" | "secondary" | "danger" | "ghost";
  size?: "sm" | "md" | "lg";
  isLoading?: boolean;
}

export const Button: React.FC<ButtonProps> = ({
  variant = "secondary",
  size = "md",
  isLoading = false,
  className = "",
  disabled,
  children,
  ...props
}) => {
  const baseStyles =
    "inline-flex items-center justify-center font-medium rounded-lg transition-colors cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed select-none";

  const sizeStyles = {
    sm: "min-h-[32px] px-3 text-xs gap-1.5",
    md: "min-h-[40px] px-4 text-[15px] gap-2",
    lg: "min-h-[44px] px-6 text-[16px] gap-2.5 font-semibold",
  };

  const variantStyles = {
    primary:
      "bg-[var(--accent-primary)] hover:bg-[var(--accent-hover)] text-white shadow-sm border border-transparent",
    secondary:
      "bg-[var(--bg-surface)] hover:bg-[var(--bg-elevated)] text-[var(--fg-primary)] border border-[var(--border)]",
    danger:
      "bg-[var(--danger)] hover:opacity-90 text-white shadow-sm border border-transparent",
    ghost:
      "bg-transparent hover:bg-[var(--bg-elevated)] text-[var(--fg-primary)] border border-transparent",
  };

  return (
    <button
      className={`${baseStyles} ${sizeStyles[size]} ${variantStyles[variant]} ${className}`}
      disabled={disabled || isLoading}
      {...props}
    >
      {isLoading ? (
        <span className="inline-block w-4 h-4 border-2 border-current border-t-transparent rounded-full animate-spin mr-2" />
      ) : null}
      {children}
    </button>
  );
};
