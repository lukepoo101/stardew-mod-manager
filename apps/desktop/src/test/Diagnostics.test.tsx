import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi, afterEach } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { DiagnosticsView } from "@/features/diagnostics/DiagnosticsView";
import { api } from "@/shared/api/client";
import type { DiagnosticsDto } from "@/shared/api/generated";
import { DEFAULT_PREFERENCES, savePreferences } from "@/shared/preferences";

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
        is_default: false,
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
        state: "installed",
        comparison: "same",
        evidence: [],
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
      log_match: "unmatched",
      log_started_at: null,
      log_read_error: null,
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
        is_default: false,
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
        state: "installed",
        comparison: "same",
        evidence: [],
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
      log_match: "unmatched",
      log_started_at: null,
      log_read_error: null,
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
      log_match: "unmatched",
      log_started_at: null,
      log_read_error: null,
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

  it("says when the log on disk is older than the latest session", async () => {
    const report = mockReport();
    const base = await (
      report.getMockImplementation() as () => Promise<DiagnosticsDto>
    )();
    report.mockResolvedValue({
      ...base,
      log_match: "stale",
      log_started_at: "2026-10-01T09:00:00Z",
    });
    renderDiagnostics();
    expect(
      await screen.findByText(/This log is older than the latest session/),
    ).toBeInTheDocument();
  });

  it("shows the end of a very large log and says why a log could not be read", async () => {
    const report = mockReport();
    const base = await (
      report.getMockImplementation() as () => Promise<DiagnosticsDto>
    )();
    const lines = Array.from({ length: 3500 }, (_, i) => `line ${i + 1}`);
    report.mockResolvedValue({
      ...base,
      raw_log: lines.join("\n"),
      log_read_error: "permission denied",
    });
    renderDiagnostics();
    expect(
      await screen.findByText(/Showing the last 2000 of 3500 lines/),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/could not be read: permission denied/),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Show all lines" }));
    expect(screen.queryByText(/Showing the last 2000/)).toBeNull();
  });

  it("speaks each finding's severity and what it means", async () => {
    mockReport();
    renderDiagnostics();
    expect(await screen.findByText("A_ERR")).toBeInTheDocument();
    expect(
      screen.getByText(". Blocks or breaks something until fixed."),
    ).toBeInTheDocument();
    expect(
      screen.getByText(". Worth checking; does not block."),
    ).toBeInTheDocument();
    expect(screen.getAllByText("Severity:").length).toBeGreaterThanOrEqual(2);
  });

  it("quiet mode hides only informational findings and says how many", async () => {
    savePreferences({ ...DEFAULT_PREFERENCES, quietInfo: true });
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
        finding("I_NOTE", "info", "log"),
      ],
      raw_log: "",
      log_file_path: "",
      host_operating_system: "linux",
      app_data_dir: "",
      cache_dir: "",
      steam_installations_checked: [],
      log_match: "unmatched",
      log_started_at: null,
      log_read_error: null,
      smapi_log_locations: [],
    });
    renderDiagnostics();
    expect(await screen.findByText("A_ERR")).toBeInTheDocument();
    expect(screen.queryByText("I_NOTE")).toBeNull();
    expect(
      screen.getByText(/1 informational finding\(s\) hidden/),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Show them" }));
    expect(screen.getByText("I_NOTE")).toBeInTheDocument();
    localStorage.clear();
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
      log_match: "unmatched",
      log_started_at: null,
      log_read_error: null,
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

  it("summarises the session and relates missing dependencies to the profile", async () => {
    vi.spyOn(api, "listProfileMods").mockResolvedValue([
      { unique_id: "Some.Base", name: "Base", version: "1.0" },
    ] as never);
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
      profile: { id: "p1", name: "D", revision: 1 },
      game: { id: "g", storefront: "Steam", operating_system: "Linux" },
      smapi_status: { is_installed: true },
      health_summary: { findings: [] },
    } as never);
    vi.spyOn(api, "getDiagnosticsReport").mockResolvedValue({
      log_summary: {
        smapi_version: "4.1.10",
        game_version: "1.6.15",
        loaded_mod_count: 3,
        skipped_mods: [
          {
            name: "Needy",
            version: "2.1",
            reason: "it needs mod Some.Base and Other.Missing",
            missing_dependencies: ["Some.Base", "Other.Missing"],
            line: 2,
          },
        ],
        update_notices: [
          {
            name: "Pretty",
            current_version: "1.0",
            available_version: "1.1",
            line: 5,
          },
        ],
        sources: [
          { source: "Pretty", errors: 2, warnings: 0, first_error_line: 4 },
        ],
        total_lines: 6,
      },
      session_id: null,
      session_state: null,
      findings: [],
      raw_log: "l1\nneedy line\nl3\nerr line\nl5\nl6",
      log_file_path: "/l",
      host_operating_system: "linux",
      app_data_dir: "/a",
      cache_dir: "/c",
      steam_installations_checked: [],
      log_match: "unmatched",
      log_started_at: null,
      log_read_error: null,
      smapi_log_locations: [],
    });
    renderDiagnostics();
    expect(
      await screen.findByText(/SMAPI 4.1.10 with Stardew Valley 1.6.15/),
    ).toBeInTheDocument();
    expect(
      await screen.findByText(/not installed in this profile/),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/installed in this profile, so check/),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/Reported by SMAPI, not checked by this manager/),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /show log line 2/i }));
    expect(screen.getByText("2: needy line")).toBeInTheDocument();
  });

  it("dismisses a warning, offers it again, and never offers errors", async () => {
    const dismiss = vi.spyOn(api, "dismissFinding").mockResolvedValue();
    vi.spyOn(api, "listDismissedFindings").mockResolvedValue([]);
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
        finding("B_WARN", "warning", "runtime"),
        finding("A_ERR", "error", "runtime"),
      ],
      raw_log: "",
      log_file_path: "/l",
      host_operating_system: "linux",
      app_data_dir: "/a",
      cache_dir: "/c",
      steam_installations_checked: [],
      log_match: "unmatched",
      log_started_at: null,
      log_read_error: null,
      smapi_log_locations: [],
    });
    renderDiagnostics();
    await screen.findByText("B_WARN summary");
    // Only the warning has a Dismiss control.
    const buttons = screen.getAllByRole("button", { name: "Dismiss" });
    expect(buttons).toHaveLength(1);
    fireEvent.click(buttons[0]);
    await waitFor(() =>
      expect(dismiss).toHaveBeenCalledWith(
        "B_WARN",
        expect.any(String),
        "warning",
        expect.objectContaining({ severity: "warning" }),
      ),
    );
  });
});
