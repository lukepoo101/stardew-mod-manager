import React, { useEffect, useState } from "react";
import { useGuardedLaunch } from "@/shared/launch/useGuardedLaunch";
import { GlobalSearch } from "@/features/search/GlobalSearch";
import { Sidebar } from "./Sidebar";
import { ContextHeader } from "./ContextHeader";
import {
  useActiveProfileOverview,
  useActiveLaunchSession,
  useTerminateSession,
} from "@/shared/api/hooks";

export const AppShell: React.FC<{ children: React.ReactNode }> = ({
  children,
}) => {
  const { data: overview } = useActiveProfileOverview();
  const { data: activeSession } = useActiveLaunchSession();
  const launchMutation = useGuardedLaunch();
  const terminateMutation = useTerminateSession();

  const [searchOpen, setSearchOpen] = useState(false);
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setSearchOpen(true);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const handleLaunch = (mode: "Modded" | "Vanilla") => {
    void launchMutation.request(mode);
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
          onSearch={() => setSearchOpen(true)}
          isLaunching={launchMutation.isPending}
        />

        <GlobalSearch open={searchOpen} onClose={() => setSearchOpen(false)} />
        {launchMutation.dialog}
        {launchMutation.error && (
          <p role="alert" className="px-6 pt-2 text-xs text-[var(--danger)]">
            {launchMutation.error}
          </p>
        )}

        <main className="flex-1 overflow-y-auto p-6 md:p-8">
          <div className="max-w-6xl mx-auto">{children}</div>
        </main>
      </div>
    </div>
  );
};
