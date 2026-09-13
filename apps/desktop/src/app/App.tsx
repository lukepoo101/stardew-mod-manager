import React, { useEffect, useState } from "react";
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
import { SmapiSetupScreen } from "@/features/setup/SmapiSetupScreen";
import { GameSelectionScreen } from "@/features/setup/GameSelectionScreen";
import { AppSnapshot } from "@/lib/backend/types";
import { backend } from "@/lib/backend/client";

const queryClient = new QueryClient();

export const AppContent: React.FC = () => {
  const [snapshot, setSnapshot] = useState<AppSnapshot | null>(null);
  const [isLoading, setIsLoading] = useState(true);

  const loadSnapshot = async () => {
    setIsLoading(true);
    try {
      const snap = await backend.getAppSnapshot();
      setSnapshot(snap);
    } catch (e) {
      console.error("Failed to load snapshot:", e);
    } finally {
      setIsLoading(false);
    }
  };

  useEffect(() => {
    loadSnapshot();
  }, []);

  if (isLoading) {
    return (
      <div className="min-h-screen flex items-center justify-center bg-[var(--bg-primary)]">
        <div className="text-center space-y-3">
          <div className="inline-block w-8 h-8 border-3 border-[var(--accent-primary)] border-t-transparent rounded-full animate-spin" />
          <p className="text-sm font-medium text-[var(--fg-muted)]">
            Loading Stardew Mod Manager...
          </p>
        </div>
      </div>
    );
  }

  if (snapshot?.recovery_error || snapshot?.active_operation) {
    return (
      <main className="p-8 space-y-4">
        <h1>Recovery required</h1>
        <p role="alert">
          {snapshot.recovery_error || "An interrupted operation needs recovery."}
        </p>
        <p>Exit the game before retrying. Your recovery journal is preserved.</p>
        <button
          onClick={async () => {
            try {
              await backend.retryRecovery();
            } finally {
              await loadSnapshot();
            }
          }}
        >
          Retry recovery
        </button>
      </main>
    );
  }

  // Setup / SMAPI onboarding check
  if (!snapshot?.selected_game) {
    return (
      <main className="max-w-4xl mx-auto p-6 md:p-8">
        <GameSelectionScreen
          onGameSelected={async (game) => {
            setIsLoading(true);
            try {
              const snap = await backend.selectGame(game.canonical_root, game.platform_kind);
              setSnapshot(snap);
            } finally {
              setIsLoading(false);
            }
          }}
        />
      </main>
    );
  }

  if (!snapshot.smapi_installed) {
    return (
      <main className="max-w-4xl mx-auto p-6 md:p-8">
        <SmapiSetupScreen
          game={snapshot.selected_game}
          onSmapiInstalled={async () => {
            await loadSnapshot();
          }}
        />
      </main>
    );
  }

  // Full modern router shell
  return (
    <AppShell>
      <Routes>
        <Route path="/" element={<OverviewView />} />
        <Route path="/onboarding" element={<OnboardingView onComplete={loadSnapshot} />} />
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
