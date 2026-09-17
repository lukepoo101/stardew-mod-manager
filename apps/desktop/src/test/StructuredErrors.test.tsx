import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ProfileModInstaller } from "@/features/mods/ProfileModInstaller";
import { api } from "@/shared/api/client";
import { ApiClientError, normalizeApiError } from "@/shared/api/errors";
import type { ApiErrorDto, OperationPreviewDto } from "@/shared/api/generated";

const preview: OperationPreviewDto = {
  operation_id: "operation-1",
  artifact_hash: "hash",
  original_filename: "ExampleMod.zip",
  byte_size: 2048,
  detected_components: [
    {
      unique_id: "Tests.Example",
      name: "Example",
      author: "Tests",
      version: "1.0.0",
      description: null,
      relative_root: "Example",
    },
  ],
  dependencies_satisfied: true,
  warnings: [],
  blockers: [],
  affected_profile_component_ids: [],
  expected_profile_revision: 7,
};

function renderInstaller() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <ProfileModInstaller profileId="profile-1" />
    </QueryClientProvider>,
  );
}

async function openPreview() {
  const input = screen.getByPlaceholderText(/Or paste path or file name/i);
  fireEvent.change(input, {
    target: { value: "/home/user/Downloads/ExampleMod.zip" },
  });
  fireEvent.click(screen.getByRole("button", { name: /Inspect/i }));
  fireEvent.click(await screen.findByRole("button", { name: "Install mod" }));
}

afterEach(() => vi.restoreAllMocks());

describe("structured error presentation", () => {
  it("shows the backend summary and offers a fresh preview when the plan is stale", async () => {
    vi.spyOn(api, "inspectPackageForInstall").mockResolvedValue(preview);
    const stalePreview: ApiErrorDto = {
      code: "PREVIEW_STALE",
      category: "operation_conflict",
      summary: "Profile was modified since the preview was generated",
      technical_details:
        "Expected profile revision 7, but current revision is 9",
      context: null,
      recoverability: "retry_with_fresh_plan",
      operation_id: null,
    };
    vi.spyOn(api, "executeOperation").mockRejectedValue(
      new ApiClientError(stalePreview),
    );

    renderInstaller();
    await openPreview();

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent(
      "Profile was modified since the preview was generated",
    );
    // A serialized Tauri error must never render as a bare object.
    expect(screen.queryByText("[object Object]")).not.toBeInTheDocument();
    // Technical details are diagnostics, not the primary message.
    expect(alert).not.toHaveTextContent("Expected profile revision 7");

    fireEvent.click(screen.getByRole("button", { name: "Refresh preview" }));

    await waitFor(() =>
      expect(api.inspectPackageForInstall).toHaveBeenCalledTimes(2),
    );
    await waitFor(() =>
      expect(screen.queryByRole("alert")).not.toBeInTheDocument(),
    );
  });

  it("does not offer a fresh preview for conflicts a refresh cannot fix", async () => {
    const conflicts: ApiErrorDto[] = [
      {
        code: "PROFILE_OPERATION_UNRESOLVED",
        category: "operation_conflict",
        summary:
          "This profile has an unresolved operation that must be reconciled before it can change",
        technical_details: "Profile 018f3b has unresolved operation 018f3a",
        context: null,
        recoverability: "requires_manual_intervention",
        operation_id: "018f3a",
      },
      {
        code: "GAME_RUNNING",
        category: "operation_conflict",
        summary: "Stardew Valley is already running",
        technical_details: "Stop Stardew Valley before changing managed files",
        context: null,
        recoverability: "retryable",
        operation_id: null,
      },
      // The shape a failed rollback produces: the diagnosis is preserved while
      // the recovery semantics say the operation needs reconciliation.
      {
        code: "FILESYSTEM_ERROR",
        category: "recovery",
        summary: "Failed to create recovery directory",
        technical_details: "Access is denied. (os error 5)",
        context: null,
        recoverability: "requires_manual_intervention",
        operation_id: "018f3a",
      },
    ];

    for (const conflict of conflicts) {
      vi.spyOn(api, "inspectPackageForInstall").mockResolvedValue(preview);
      vi.spyOn(api, "executeOperation").mockRejectedValue(
        new ApiClientError(conflict),
      );

      const { unmount } = renderInstaller();
      await openPreview();

      const alert = await screen.findByRole("alert");
      expect(alert).toHaveTextContent(conflict.summary);
      // Refreshing the preview does not resolve these, so the stale-plan
      // recovery must not be offered.
      expect(
        screen.queryByRole("button", { name: "Refresh preview" }),
      ).not.toBeInTheDocument();

      unmount();
      vi.restoreAllMocks();
    }
  });

  it("does not offer a stale-plan recovery for terminal failures", async () => {
    vi.spyOn(api, "inspectPackageForInstall").mockResolvedValue(preview);
    vi.spyOn(api, "executeOperation").mockRejectedValue(
      new ApiClientError({
        code: "INVALID_OPERATION_ID",
        category: "validation",
        summary: "The operation identifier is invalid",
        technical_details: "invalid character: 'x'",
        context: null,
        recoverability: "terminal",
        operation_id: null,
      }),
    );

    renderInstaller();
    await openPreview();

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("The operation identifier is invalid");
    expect(
      screen.queryByRole("button", { name: "Refresh preview" }),
    ).not.toBeInTheDocument();
  });

  it("renders a normalized unexpected rejection as a safe generic message", async () => {
    vi.spyOn(api, "inspectPackageForInstall").mockResolvedValue(preview);
    // What the invoke wrapper produces for a framework/runtime failure.
    vi.spyOn(api, "executeOperation").mockRejectedValue(
      normalizeApiError("kernel exploded"),
    );

    renderInstaller();
    await openPreview();

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent(
      "An unexpected desktop communication error occurred",
    );
    expect(alert).not.toHaveTextContent("kernel exploded");
  });
});
