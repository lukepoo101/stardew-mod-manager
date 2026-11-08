import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "bun:test";
import { GitHubTokenCard } from "@/features/settings/GitHubTokenCard";
import { api } from "@/shared/api/client";

afterEach(() => vi.restoreAllMocks());

const show = () =>
  render(
    <QueryClientProvider client={new QueryClient()}>
      <GitHubTokenCard />
    </QueryClientProvider>,
  );

describe("the GitHub token", () => {
  it("saves a pasted token and forgets it in the window", async () => {
    vi.spyOn(api, "getGitHubTokenStatus").mockResolvedValue({
      set: false,
      rejected: false,
      keychain_error: null,
    });
    const set = vi.spyOn(api, "setGitHubToken").mockResolvedValue({
      set: true,
      rejected: false,
      keychain_error: null,
    });
    show();
    await screen.findByText("No token is saved.");
    const input = screen.getByLabelText("Paste a token") as HTMLInputElement;
    expect(input.type).toBe("password");
    fireEvent.change(input, { target: { value: "github_pat_abc" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await screen.findByText("A token is saved in your system keychain.");
    expect(set).toHaveBeenCalledWith("github_pat_abc");
    expect(
      (screen.getByLabelText("Replace the token") as HTMLInputElement).value,
    ).toBe("");
  });

  it("tests the saved token and shows the limit", async () => {
    vi.spyOn(api, "getGitHubTokenStatus").mockResolvedValue({
      set: true,
      rejected: false,
      keychain_error: null,
    });
    const test = vi.spyOn(api, "testGitHubToken").mockResolvedValue({
      with_token: true,
      limit: 5000,
      remaining: 4990,
      resets_at: null,
    });
    show();
    await screen.findByText("A token is saved in your system keychain.");
    fireEvent.click(screen.getByRole("button", { name: "Test" }));
    await screen.findByText(
      "With this token GitHub allows 5,000 requests an hour; 4,990 are left.",
    );
    expect(test).toHaveBeenCalledWith(null);
  });

  it("says when GitHub rejected the saved token", async () => {
    vi.spyOn(api, "getGitHubTokenStatus").mockResolvedValue({
      set: true,
      rejected: true,
      keychain_error: null,
    });
    show();
    await waitFor(() =>
      expect(screen.getByRole("status").textContent).toContain(
        "GitHub rejected it",
      ),
    );
  });
});
