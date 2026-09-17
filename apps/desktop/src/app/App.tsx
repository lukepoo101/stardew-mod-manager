import React, { useState } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { HashRouter, Routes, Route, Link } from "react-router-dom";
import { ThemeProvider } from "@/shared/theme/ThemeProvider";
import { AppShell } from "@/components/layout/AppShell";
import { OnboardingView } from "@/features/onboarding/OnboardingView";
import { OverviewView } from "@/features/overview/OverviewView";
import { ModsView } from "@/features/mods/ModsView";
import { ProfilesView } from "@/features/profiles/ProfilesView";
import { DiagnosticsView } from "@/features/diagnostics/DiagnosticsView";
import { ActivityView } from "@/features/activity/ActivityView";
import { SettingsView } from "@/features/settings/SettingsView";
import { useBootstrap } from "@/shared/api/hooks";
import { api } from "@/shared/api/client";
import { skipOnboarding } from "@/shared/api/onboarding";
import { errorSummary } from "@/shared/api/errors";

const EmptyWorkspace: React.FC = () => (
  <div className="max-w-2xl space-y-5 py-10">
    <div className="space-y-2">
      <h2 className="text-2xl font-extrabold tracking-tight">
        No managed game configured
      </h2>
      <p className="text-sm text-[var(--fg-muted)] leading-relaxed">
        You skipped setup, so Stardew Mod Manager has not changed any game
        files. You can use Settings now or return to guided setup when you are
        ready.
      </p>
    </div>
    <div className="flex flex-wrap gap-3">
      <Link
        to="/onboarding"
        className="px-4 py-2 rounded-lg bg-[var(--accent-primary)] text-white text-sm font-semibold"
      >
        Run guided setup
      </Link>
      <Link
        to="/app/settings"
        className="px-4 py-2 rounded-lg border border-[var(--border)] bg-[var(--bg-surface)] text-sm font-semibold"
      >
        Open settings
      </Link>
    </div>
  </div>
);

export const AppContent: React.FC = () => {
  const { data: bootstrap, isLoading, error, refetch } = useBootstrap();
  const [recoveryError, setRecoveryError] = useState<string | null>(null);
  const [isSkipping, setIsSkipping] = useState(false);

  if (isLoading) {
    return (
      <main className="p-8" role="status">
        Loading Stardew Mod Manager...
      </main>
    );
  }
  if (error || !bootstrap) {
    return (
      <main className="p-8 space-y-4">
        <h1>Unable to load Stardew Mod Manager</h1>
        <p role="alert">
          {error ? errorSummary(error) : "No startup data returned"}
        </p>
        <button onClick={() => void refetch()}>Retry</button>
      </main>
    );
  }
  if (bootstrap.recovery_summary) {
    return (
      <main className="p-8 space-y-4">
        <h1>Recovery required</h1>
        <p role="alert">{recoveryError || bootstrap.recovery_summary}</p>
        <button
          onClick={async () => {
            setRecoveryError(null);
            try {
              await api.retryRecovery();
              await refetch();
            } catch (error) {
              setRecoveryError(errorSummary(error));
            }
          }}
        >
          Retry recovery
        </button>
      </main>
    );
  }

  const needsOnboarding =
    bootstrap.onboarding_disposition === "not_started" ||
    (bootstrap.onboarding_disposition === "completed" &&
      !bootstrap.active_profile_id);

  if (needsOnboarding) {
    return (
      <main className="max-w-4xl mx-auto p-6 md:p-8 space-y-4">
        <OnboardingView
          initialGameId={bootstrap.active_game_installation_id ?? undefined}
          onComplete={async () => {
            await refetch();
          }}
        />
        {bootstrap.onboarding_disposition === "not_started" && (
          <div className="flex justify-center border-t border-[var(--border)] pt-4">
            <button
              type="button"
              disabled={isSkipping}
              className="text-sm text-[var(--fg-muted)] hover:text-[var(--fg-primary)] underline underline-offset-4 disabled:opacity-50"
              onClick={async () => {
                setIsSkipping(true);
                try {
                  await skipOnboarding();
                  await refetch();
                } finally {
                  setIsSkipping(false);
                }
              }}
            >
              {isSkipping ? "Skipping setup…" : "Skip setup for now"}
            </button>
          </div>
        )}
      </main>
    );
  }

  const hasActiveProfile = Boolean(bootstrap.active_profile_id);
  const withoutWorkspace = <EmptyWorkspace />;

  return (
    <AppShell>
      <Routes>
        <Route
          path="/"
          element={hasActiveProfile ? <OverviewView /> : withoutWorkspace}
        />
        <Route
          path="/onboarding"
          element={
            <OnboardingView
              onComplete={async () => {
                await refetch();
              }}
            />
          }
        />
        <Route
          path="/app/overview"
          element={hasActiveProfile ? <OverviewView /> : withoutWorkspace}
        />
        <Route
          path="/app/mods"
          element={hasActiveProfile ? <ModsView /> : withoutWorkspace}
        />
        <Route
          path="/app/profiles"
          element={hasActiveProfile ? <ProfilesView /> : withoutWorkspace}
        />
        <Route
          path="/app/diagnostics"
          element={hasActiveProfile ? <DiagnosticsView /> : withoutWorkspace}
        />
        <Route
          path="/app/activity"
          element={hasActiveProfile ? <ActivityView /> : withoutWorkspace}
        />
        <Route path="/app/settings" element={<SettingsView />} />
      </Routes>
    </AppShell>
  );
};

export const App: React.FC = () => {
  const [queryClient] = useState(
    () => new QueryClient({ defaultOptions: { queries: { retry: false } } }),
  );
  return (
    <QueryClientProvider client={queryClient}>
      <ThemeProvider>
        <HashRouter>
          <AppContent />
        </HashRouter>
      </ThemeProvider>
    </QueryClientProvider>
  );
};
