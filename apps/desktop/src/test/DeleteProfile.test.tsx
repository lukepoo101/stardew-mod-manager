import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { DeleteProfileDialog } from "@/features/profiles/DeleteProfileDialog";
import { api } from "@/shared/api/client";

afterEach(() => vi.restoreAllMocks());

describe("deleting a profile", () => {
  it("lists what is removed and kept, and needs the name typed", async () => {
    vi.spyOn(api, "previewProfileDeletion").mockResolvedValue({
      profile_id: "p2",
      name: "Old save",
      mod_count: 12,
      folder_bytes: 3 * 1024 * 1024,
      packages_kept: 9,
      blocked_reason: null,
    });
    const remove = vi.spyOn(api, "deleteProfile").mockResolvedValue();
    const onClose = vi.fn();
    render(<DeleteProfileDialog profileId="p2" onClose={onClose} />);

    expect(
      await screen.findByText(/its 12 installed mod\(s\)/),
    ).toBeInTheDocument();
    expect(screen.getByText(/3.0 MB/)).toBeInTheDocument();
    expect(
      screen.getByText(/The 9 downloaded package\(s\)/),
    ).toBeInTheDocument();
    const button = screen.getByRole("button", { name: "Delete permanently" });
    expect(button).toBeDisabled();

    fireEvent.change(screen.getByRole("textbox"), {
      target: { value: "Old save" },
    });
    expect(button).toBeEnabled();
    fireEvent.click(button);
    await waitFor(() => expect(remove).toHaveBeenCalledWith("p2"));
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("explains why a profile cannot be deleted yet", async () => {
    vi.spyOn(api, "previewProfileDeletion").mockResolvedValue({
      profile_id: "p1",
      name: "Main",
      mod_count: 1,
      folder_bytes: 10,
      packages_kept: 1,
      blocked_reason: "Archive the profile before deleting it.",
    });
    render(<DeleteProfileDialog profileId="p1" onClose={vi.fn()} />);
    expect(
      await screen.findByText("Archive the profile before deleting it."),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Delete permanently" }),
    ).toBeDisabled();
  });
});
