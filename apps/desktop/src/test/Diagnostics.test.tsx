import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi, afterEach } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { DiagnosticsView } from "@/features/diagnostics/DiagnosticsView";
import { api } from "@/shared/api/client";

const renderDiagnostics = () =>
  render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <DiagnosticsView />
    </QueryClientProvider>,
  );

afterEach(() => vi.restoreAllMocks());

describe("diagnostics report", () => {
  it("renders the SMAPI log, its path and reported findings", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValueOnce({
      profile: {
        id: "profile-1",
        game_installation_id: "game-1",
        name: "Default",
        description: null,
        revision: 1,
        mod_count: 1,
        created_at: new Date().toISOString(),
        updated_at: new Date().toISOString(),
        state: "Active",
      },
      game: {
        id: "game-1",
        canonical_root: "/games/Stardew Valley",
        operating_system: "Linux",
        storefront: "Steam",
        management_mode: "Managed",
        created_at: new Date().toISOString(),
      },
      mod_count: 1,
      smapi_status: {
        is_installed: true,
        observed_version: "4.1.10",
        tested_version: "4.1.10",
        is_compatible: true,
      },
      health_summary: {
        status: "Warning",
        warning_count: 1,
        error_count: 0,
        info_count: 0,
        findings: [],
      },
      last_session: null,
    });
    const report = vi.spyOn(api, "getDiagnosticsReport").mockResolvedValue({
      log_summary: {
        smapi_version: null,
        game_version: null,
        loaded_mod_count: null,
        skipped_mods: [],
        update_notices: [],
        sources: [],
        total_lines: 0,
      },
      session_id: null,
      session_state: null,
      findings: [
        {
          id: "finding-1",
          fingerprint: "mod-load-unverified",
          code: "MOD_LOAD_UNVERIFIED",
          severity: "Warning",
          category: "Launch",
          title: "Mod load unverified",
          summary: "Mod load could not be verified from the SMAPI log",
          affected_entities: [],
          evidence: [],
          observed_at: new Date().toISOString(),
        },
      ],
      raw_log: "[12:00:00 TRACE SMAPI] Test Mod 1.0.0 by Author",
      log_file_path: "/logs/SMAPI-latest.txt",
      host_operating_system: "windows",
      app_data_dir: "C:\\Users\\tester\\AppData\\Roaming\\stardew-mod-manager",
      cache_dir: "C:\\Users\\tester\\AppData\\Local\\stardew-mod-manager",
      steam_installations_checked: ["C:\\Program Files (x86)\\Steam"],
      smapi_log_locations: [
        {
          operating_system: "windows",
          context: "SMAPI log (Windows)",
          path: "%APPDATA%\\StardewValley\\ErrorLogs\\SMAPI-latest.txt",
        },
      ],
    });

    renderDiagnostics();

    // The log appears in the log viewer and, redacted, in the support preview.
    expect(
      (await screen.findAllByText(/Test Mod 1.0.0 by Author/)).length,
    ).toBeGreaterThanOrEqual(1);
    expect(screen.getByText("/logs/SMAPI-latest.txt")).toBeInTheDocument();
    expect(screen.getByText("MOD_LOAD_UNVERIFIED")).toBeInTheDocument();
    expect(
      screen.getByText("Mod load could not be verified from the SMAPI log"),
    ).toBeInTheDocument();
    expect(report).toHaveBeenCalled();
  });

  it("reports the host platform and where the manager looked", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
      profile: {
        id: "profile-1",
        game_installation_id: "game-1",
        name: "Default",
        description: null,
        revision: 1,
        mod_count: 0,
        created_at: new Date().toISOString(),
        updated_at: new Date().toISOString(),
        state: "Active",
      },
      game: {
        id: "game-1",
        canonical_root: "C:\\Games\\Stardew Valley",
        operating_system: "windows",
        storefront: "steam",
        management_mode: "managed",
        created_at: new Date().toISOString(),
      },
      mod_count: 0,
      smapi_status: {
        is_installed: true,
        observed_version: "4.1.10",
        tested_version: "4.1.10",
        is_compatible: true,
      },
      health_summary: {
        status: "Healthy",
        warning_count: 0,
        error_count: 0,
        info_count: 0,
        findings: [],
      },
      last_session: null,
    });
    vi.spyOn(api, "getDiagnosticsReport").mockResolvedValue({
      log_summary: {
        smapi_version: null,
        game_version: null,
        loaded_mod_count: null,
        skipped_mods: [],
        update_notices: [],
        sources: [],
        total_lines: 0,
      },
      session_id: null,
      session_state: null,
      findings: [],
      raw_log: "",
      log_file_path:
        "C:\\Users\\tester\\AppData\\Roaming\\StardewValley\\ErrorLogs\\SMAPI-latest.txt",
      host_operating_system: "windows",
      app_data_dir: "C:\\Users\\tester\\AppData\\Roaming\\stardew-mod-manager",
      cache_dir: "C:\\Users\\tester\\AppData\\Local\\stardew-mod-manager",
      steam_installations_checked: ["C:\\Program Files (x86)\\Steam"],
      smapi_log_locations: [
        {
          operating_system: "windows",
          context: "SMAPI log (Windows)",
          path: "%APPDATA%\\StardewValley\\ErrorLogs\\SMAPI-latest.txt",
        },
      ],
    });

    renderDiagnostics();

    // The platform label is the backend's answer, not a UI guess.
    expect(await screen.findByText("Windows")).toBeInTheDocument();
    expect(
      screen.getByText("C:\\Program Files (x86)\\Steam"),
    ).toBeInTheDocument();
    expect(screen.getByText("SMAPI log (Windows)")).toBeInTheDocument();
  });
});

describe("support export and findings filter", () => {
  const finding = (code: string, severity: string, category: string) => ({
    id: code,
    fingerprint: code,
    code,
    severity,
    category,
    title: `${code} title`,
    summary: `${code} summary`,
    affected_entities: [],
    evidence: [],
    observed_at: new Date().toISOString(),
  });

  const mockReport = () =>
    vi.spyOn(api, "getDiagnosticsReport").mockResolvedValue({
      log_summary: {
        smapi_version: null,
        game_version: null,
        loaded_mod_count: null,
        skipped_mods: [],
        update_notices: [],
        sources: [],
        total_lines: 0,
      },
      session_id: null,
      session_state: null,
      findings: [
        finding("A_ERR", "error", "runtime"),
        finding("B_WARN", "Warning", "launch"),
      ],
      raw_log: "boom at /home/luke/Mods token=abcdefgh12345678",
      log_file_path: "/home/luke/log.txt",
      host_operating_system: "linux",
      app_data_dir: "/home/luke/.local/share/x",
      cache_dir: "/home/luke/.cache/x",
      steam_installations_checked: [],
      smapi_log_locations: [],
    });

  it("filters findings by structured severity regardless of case and can clear", async () => {
    mockReport();
    renderDiagnostics();
    expect(await screen.findByText("A_ERR")).toBeInTheDocument();
    expect(screen.getByText("B_WARN")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /warning \(1\)/ }));
    expect(screen.queryByText("A_ERR")).not.toBeInTheDocument();
    expect(screen.getByText("B_WARN")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /clear filters/i }));
    expect(screen.getByText("A_ERR")).toBeInTheDocument();
  });

  it("previews a redacted summary and never shows the original secret", async () => {
    mockReport();
    renderDiagnostics();
    const preview = (await screen.findByLabelText(
      "Support summary preview",
    )) as HTMLTextAreaElement;
    await waitFor(() => expect(preview.value).toContain("[error] A_ERR"));
    expect(preview.value).toContain("~/Mods");
    expect(preview.value).not.toContain("abcdefgh12345678");
    expect(preview.value).not.toContain("/home/luke");
  });

  it("copies the redacted log by default", async () => {
    mockReport();
    const write = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText: write },
      configurable: true,
    });
    renderDiagnostics();
    await screen.findByText("A_ERR");
    fireEvent.click(
      screen.getByRole("button", { name: /copy log \(redacted\)/i }),
    );
    await waitFor(() => expect(write).toHaveBeenCalledTimes(1));
    expect(write.mock.calls[0][0]).not.toContain("abcdefgh12345678");
    expect(write.mock.calls[0][0]).toContain("~/Mods");
  });

  it("searches the log by line and shows evidence behind a finding", async () => {
    vi.spyOn(api, "getDiagnosticsReport").mockResolvedValue({
      session_id: null,
      session_state: null,
      findings: [
        {
          ...finding("A_ERR", "error", "runtime"),
          evidence: ["seen in /home/luke/log token=abcdefgh12345678"],
        },
      ],
      raw_log: "one\ntwo needle\nthree",
      log_file_path: "/l",
      host_operating_system: "linux",
      app_data_dir: "/a",
      cache_dir: "/c",
      steam_installations_checked: [],
      smapi_log_locations: [],
    });
    renderDiagnostics();
    await screen.findByText("A_ERR");
    expect(screen.getByText(/seen in ~\/log/)).toBeInTheDocument();
    expect(screen.queryByText(/abcdefgh12345678/)).toBeNull();

    fireEvent.change(screen.getByLabelText("Search log"), {
      target: { value: "needle" },
    });
    expect(screen.getByText("1 of 3 line(s) match")).toBeInTheDocument();
    expect(screen.getByText("2: two needle")).toBeInTheDocument();
  });
});
