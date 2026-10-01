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
    const saves = await screen.findByLabelText("Backups kept for each save");
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
});
