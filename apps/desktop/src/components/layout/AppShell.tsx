import React, { useEffect, useState } from "react";
import { useGuardedLaunch } from "@/shared/launch/useGuardedLaunch";
import { GlobalSearch } from "@/features/search/GlobalSearch";
import { Sidebar } from "./Sidebar";
import { ContextHeader } from "./ContextHeader";
import { SmapiUpdateNotice } from "@/features/smapi/SmapiUpdateNotice";
import { StartupRecoveryNotice } from "@/features/recovery/StartupRecoveryNotice";
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
        // Not over an open dialog: leaving a review that way skipped its
        // Cancel, and its prepared change kept the profile locked.
        if (document.querySelector('[role="dialog"], [role="alertdialog"]'))
          return;
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
    // A size container, so the shell's breakpoints are measured in the
    // scaled rem: media queries ignore the interface size setting.
    <div className="@container min-h-screen flex bg-[var(--bg-primary)] text-[var(--fg-primary)] overflow-hidden">
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

        <SmapiUpdateNotice />
        <StartupRecoveryNotice />

        {/* Focusable so focus has somewhere to land inside the page when the
            control that opened a dialog is gone. */}
        <main
          id="main-content"
          tabIndex={-1}
          className="@container flex-1 overflow-y-auto p-4 @3xl:p-8 focus:outline-none"
        >
          <div className="max-w-6xl mx-auto min-w-0">{children}</div>
        </main>
      </div>
    </div>
  );
};
