import React, { useState } from "react";
import { QueryClient, QueryClientProvider } from "@/shared/api/query";
import { HashRouter, Routes, Route } from "@/shared/router";
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
import { backend } from "@/shared/api/client";

export const AppContent: React.FC = () => {
  const { data: bootstrap, isLoading, error, refetch } = useBootstrap();
  const [recoveryError, setRecoveryError] = useState<string | null>(null);

  if (isLoading) {
    return <main className="p-8" role="status">Loading Stardew Mod Manager...</main>;
  }
  if (error || !bootstrap) {
    return <main className="p-8 space-y-4">
      <h1>Unable to load Stardew Mod Manager</h1>
      <p role="alert">{error?.message || "No startup data returned"}</p>
      <button onClick={() => void refetch()}>Retry</button>
    </main>;
  }
  if (bootstrap.recovery_summary) {
    return <main className="p-8 space-y-4">
      <h1>Recovery required</h1>
      <p role="alert">{recoveryError || bootstrap.recovery_summary}</p>
      <button onClick={async () => {
        setRecoveryError(null);
        try { await backend.retryRecovery(); await refetch(); }
        catch (error) { setRecoveryError(String(error)); }
      }}>Retry recovery</button>
    </main>;
  }
  if (!bootstrap.active_profile_id || bootstrap.onboarding_disposition !== "completed") {
    return <main className="max-w-4xl mx-auto p-6 md:p-8">
      <OnboardingView initialGameId={bootstrap.active_game_installation_id ?? undefined} onComplete={async () => { await refetch(); }} />
    </main>;
  }

  // Full modern router shell
  return (
    <AppShell>
      <Routes>
        <Route path="/" element={<OverviewView />} />
        <Route path="/onboarding" element={<OnboardingView onComplete={async () => { await refetch(); }} />} />
        <Route path="/app/overview" element={<OverviewView />} />
        <Route path="/app/mods" element={<ModsView />} />
        <Route path="/app/profiles" element={<ProfilesView />} />
        <Route path="/app/diagnostics" element={<DiagnosticsView />} />
        <Route path="/app/activity" element={<ActivityView />} />
        <Route path="/app/settings" element={<SettingsView />} />
      </Routes>
    </AppShell>
  );
};

export const App: React.FC = () => {
  const [queryClient] = useState(() => new QueryClient());
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
