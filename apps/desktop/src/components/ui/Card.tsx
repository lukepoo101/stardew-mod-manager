import React from "react";

export interface CardProps extends React.HTMLAttributes<HTMLDivElement> {
  children: React.ReactNode;
  className?: string;
}

export const Card: React.FC<CardProps> = ({ children, className = "", ...props }) => {
  return (
    <div
      className={`bg-[var(--bg-surface)] border border-[var(--border)] rounded-xl p-5 shadow-xs ${className}`}
      {...props}
    >
      {children}
    </div>
  );
};
