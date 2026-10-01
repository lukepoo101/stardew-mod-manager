import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { CloneProfileDialog } from "@/features/profiles/CloneProfileDialog";
import { api } from "@/shared/api/client";
import type { ProfileSummaryDto } from "@/shared/api/generated";

afterEach(() => vi.restoreAllMocks());

const profile = { id: "p1", name: "Main" } as ProfileSummaryDto;

describe("duplicating a profile", () => {
  it("names the copy and reports what was copied and what was not", async () => {
    const clone = vi.spyOn(api, "cloneProfile").mockResolvedValue({
      settings_applied: [],
      profile_id: "p2",
      profile_name: "Main test",
      installed: ["A 1.0", "B 2.0"],
      disabled: ["B"],
      failures: [{ name: "Old", reason: "The package is no longer stored" }],
    });
    render(<CloneProfileDialog profile={profile} onClose={vi.fn()} />);
    const input = screen.getByLabelText("Name for the copy");
    expect(input).toHaveValue("Main copy");
    fireEvent.change(input, { target: { value: "Main test" } });
    fireEvent.click(screen.getByRole("button", { name: "Duplicate" }));
    await waitFor(() => expect(clone).toHaveBeenCalledWith("p1", "Main test"));
    expect(
      await screen.findByText(/Created "Main test" with 2 mod\(s\)/),
    ).toBeInTheDocument();
    expect(screen.getByText(/"Main" is unchanged/)).toBeInTheDocument();
    expect(
      screen.getByText("Old: The package is no longer stored"),
    ).toBeInTheDocument();
  });

  it("keeps the dialog open on a failure", async () => {
    vi.spyOn(api, "cloneProfile").mockRejectedValue(
      new Error("A profile named 'Main copy' already exists"),
    );
    render(<CloneProfileDialog profile={profile} onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Duplicate" }));
    expect(await screen.findByRole("alert")).toBeInTheDocument();
  });
});
