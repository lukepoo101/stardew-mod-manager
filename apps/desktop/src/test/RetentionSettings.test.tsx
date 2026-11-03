import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { RetentionSettings } from "@/features/settings/RetentionSettings";
import { api } from "@/shared/api/client";

afterEach(() => vi.restoreAllMocks());

describe("retention settings", () => {
  it("shows the current limits and saves a change", async () => {
    vi.spyOn(api, "getRetentionPolicy").mockResolvedValue({
      keep_save_backups: 5,
      keep_settings_backups: 5,
      keep_trash_days: 30,
    });
    const save = vi
      .spyOn(api, "setRetentionPolicy")
      .mockImplementation(async (policy) => policy);
    const onSaved = vi.fn();
    render(<RetentionSettings onSaved={onSaved} />);
    const saves = await screen.findByLabelText(/Backups kept for each save/);
    expect(saves).toHaveValue(5);
    fireEvent.change(saves, { target: { value: "2" } });
    fireEvent.click(screen.getByRole("button", { name: "Save limits" }));
    await waitFor(() =>
      expect(save).toHaveBeenCalledWith({
        keep_save_backups: 2,
        keep_settings_backups: 5,
        keep_trash_days: 30,
      }),
    );
    expect(
      await screen.findByText(/next check uses these limits/),
    ).toBeInTheDocument();
    expect(onSaved).toHaveBeenCalled();
  });

  it("says why an invalid limit is not saved and clears the old status", async () => {
    vi.spyOn(api, "getRetentionPolicy").mockResolvedValue({
      keep_save_backups: 5,
      keep_settings_backups: 5,
      keep_trash_days: 30,
    });
    const save = vi
      .spyOn(api, "setRetentionPolicy")
      .mockImplementation(async (policy) => policy);
    render(<RetentionSettings />);
    fireEvent.click(await screen.findByRole("button", { name: "Save limits" }));
    expect(await screen.findByText(/next check uses/)).toBeInTheDocument();
    expect(save).toHaveBeenCalledTimes(1);

    for (const value of ["0", "101", "2.5", ""]) {
      fireEvent.change(screen.getByLabelText(/Backups kept for each save/), {
        target: { value },
      });
      expect(screen.queryByText(/next check uses/)).toBeNull();
      fireEvent.click(screen.getByRole("button", { name: "Save limits" }));
      expect(await screen.findByRole("alert")).toHaveTextContent(
        "Nothing was saved. Enter a whole number: Backups kept for each save from 1 to 100.",
      );
    }
    fireEvent.change(screen.getByLabelText(/Days a deleted profile/), {
      target: { value: "3651" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save limits" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Days a deleted profile stays in the trash from 1 to 3650",
    );
    expect(save).toHaveBeenCalledTimes(1);
  });
});
