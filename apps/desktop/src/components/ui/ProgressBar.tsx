import React from "react";

export interface ProgressBarProps {
  stages: string[];
  currentStageIndex: number;
}

export const ProgressBar: React.FC<ProgressBarProps> = ({
  stages,
  currentStageIndex,
}) => {
  return (
    <div className="space-y-2">
      <div className="flex justify-between text-xs font-semibold text-[var(--fg-muted)]">
        {stages.map((stage, idx) => {
          const isCompleted = idx < currentStageIndex;
          const isCurrent = idx === currentStageIndex;
          return (
            <span
              key={stage}
              className={`${
                isCurrent
                  ? "text-[var(--accent-primary)] font-bold"
                  : isCompleted
                    ? "text-[var(--fg-primary)]"
                    : ""
              }`}
            >
              {stage}
            </span>
          );
        })}
      </div>
      <div className="w-full bg-[var(--bg-elevated)] rounded-full h-2 overflow-hidden border border-[var(--border)]">
        <div
          className="bg-[var(--accent-primary)] h-full transition-all duration-300 rounded-full"
          style={{
            width: `${((currentStageIndex + 1) / stages.length) * 100}%`,
          }}
        />
      </div>
    </div>
  );
};
