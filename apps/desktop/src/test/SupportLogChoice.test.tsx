import { fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SupportExportCard } from "@/features/diagnostics/SupportExportCard";
import { api } from "@/shared/api/client";
import type { LaunchSessionDto } from "@/shared/api/generated";

afterEach(() => vi.restoreAllMocks());

describe("choosing the log for a support export", () => {
  it("uses a session's saved log, and says when there is none", async () => {
    vi.spyOn(api, "listLaunchSessions").mockResolvedValue([
      {
        id: "s-old",
        launched_at: "2026-10-01T09:00:00Z",
      } as unknown as LaunchSessionDto,
    ]);
    vi.spyOn(api, "listSavedSessionLogs").mockResolvedValue(["s-old"]);
    const base = await api.getDiagnosticsReport();
    const fetch = vi.spyOn(api, "getDiagnosticsReport").mockResolvedValue({
      ...base,
      raw_log: "[09:00:00 ERROR Old] the old problem",
      log_is_saved_copy: true,
    });
    render(
      <QueryClientProvider client={new QueryClient()}>
        <SupportExportCard report={base} overview={undefined} mods={[]} />
      </QueryClientProvider>,
    );
    fireEvent.change(await screen.findByLabelText("Log to include"), {
      target: { value: "s-old" },
    });
    expect(await screen.findByText(/the old problem/)).toBeInTheDocument();
    expect(fetch).toHaveBeenCalledWith(undefined, "s-old");
    expect(
      screen.getAllByText(/from the saved log of the session started/).length,
    ).toBeGreaterThan(0);
  });

  it("offers only sessions whose log is still kept", async () => {
    vi.spyOn(api, "listLaunchSessions").mockResolvedValue([
      { id: "kept", launched_at: "2026-10-02T09:00:00Z" },
      { id: "pruned", launched_at: "2026-10-01T09:00:00Z" },
    ] as unknown as LaunchSessionDto[]);
    vi.spyOn(api, "listSavedSessionLogs").mockResolvedValue(["kept"]);
    const base = await api.getDiagnosticsReport();
    render(
      <QueryClientProvider client={new QueryClient()}>
        <SupportExportCard report={base} overview={undefined} mods={[]} />
      </QueryClientProvider>,
    );
    const select = (await screen.findByLabelText(
      "Log to include",
    )) as HTMLSelectElement;
    expect(
      screen.getByText(/Only sessions whose log the manager still keeps/),
    ).toBeInTheDocument();
    expect([...select.options].map((o) => o.value)).toEqual(["", "kept"]);
  });

  it("asks for a folder, saves there and says where it went", async () => {
    vi.spyOn(api, "listLaunchSessions").mockResolvedValue([]);
    localStorage.removeItem("smm.share-folder");
    const base = await api.getDiagnosticsReport();
    const choose = vi
      .spyOn(api, "chooseFolderDialog")
      .mockResolvedValue("/home/me/Desktop");
    const save = vi
      .spyOn(api, "saveJsonExport")
      .mockResolvedValue("/home/me/Desktop/support-bundle-x.json");
    render(
      <QueryClientProvider client={new QueryClient()}>
        <SupportExportCard report={base} overview={undefined} mods={[]} />
      </QueryClientProvider>,
    );
    const button = await screen.findByRole("button", { name: "Save bundle" });
    if (button.hasAttribute("disabled")) {
      fireEvent.click(screen.getByRole("checkbox", { name: /I have read/ }));
    }
    fireEvent.click(button);
    expect(
      await screen.findByText(
        "Bundle saved to /home/me/Desktop/support-bundle-x.json.",
      ),
    ).toBeInTheDocument();
    expect(choose).toHaveBeenCalledWith(
      "Choose where to save the support bundle",
    );
    expect(save.mock.calls[0][0]).toBe("/home/me/Desktop");
    expect(save.mock.calls[0][1]).toMatch(/^support-bundle-.*\.json$/);
  });
});
