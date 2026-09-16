import React from "react";
import { Sidebar } from "./Sidebar";
import { ContextHeader } from "./ContextHeader";
import {
  useActiveProfileOverview,
  useActiveLaunchSession,
  useLaunchGame,
  useTerminateSession,
} from "@/shared/api/hooks";

export const AppShell: React.FC<{ children: React.ReactNode }> = ({
  children,
}) => {
  const { data: overview } = useActiveProfileOverview();
  const { data: activeSession } = useActiveLaunchSession();
  const launchMutation = useLaunchGame();
  const terminateMutation = useTerminateSession();

  const handleLaunch = () => {
    launchMutation.mutate("Modded");
  };

  const handleTerminate = () => {
    terminateMutation.mutate(activeSession?.id);
  };

  return (
    <div className="min-h-screen flex bg-[var(--bg-primary)] text-[var(--fg-primary)] overflow-hidden">
      {/* Fixed Sidebar */}
      <Sidebar
        activeProfileName={overview?.profile.name}
        activeProfileRevision={
          overview?.profile.revision !== undefined
            ? Number(overview.profile.revision)
            : undefined
        }
      />

      {/* Main Content Area */}
      <div className="flex-1 flex flex-col min-w-0 h-screen overflow-hidden">
        <ContextHeader
          overview={overview}
          activeSession={activeSession}
          onLaunch={handleLaunch}
          onTerminate={handleTerminate}
          isLaunching={launchMutation.isPending}
        />

        <main className="flex-1 overflow-y-auto p-6 md:p-8">
          <div className="max-w-6xl mx-auto">{children}</div>
        </main>
      </div>
    </div>
  );
};
