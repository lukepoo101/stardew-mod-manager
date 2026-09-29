import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { ModRelationsPanel } from "@/features/mods/ModRelationsPanel";
import { api } from "@/shared/api/client";

afterEach(() => vi.restoreAllMocks());

describe("mod relations", () => {
  it("explains the reason, the chain to a gap and who depends on it", async () => {
    vi.spyOn(api, "getModRelations").mockResolvedValue({
      installed_reason: "dependency",
      reason_detail: "It was installed because Top needs it.",
      requires: [
        {
          unique_id: "A.Missing",
          name: null,
          installed_version: null,
          minimum_version: "2.0",
          kind: "required",
          status: "missing",
        },
        {
          unique_id: "A.Nice",
          name: null,
          installed_version: null,
          minimum_version: null,
          kind: "optional",
          status: "missing",
        },
      ],
      required_by: [
        {
          profile_component_id: "c1",
          name: "Top",
          unique_id: "A.Top",
          enabled: true,
          minimum_version: null,
          kind: "required",
        },
      ],
      broken_chains: [["Framework", "A.Missing"]],
    });
    render(
      <QueryClientProvider client={new QueryClient()}>
        <ModRelationsPanel profileComponentId="c2" />
      </QueryClientProvider>,
    );
    expect(
      await screen.findByText("It was installed because Top needs it."),
    ).toBeInTheDocument();
    expect(screen.getByText("Framework → A.Missing")).toBeInTheDocument();
    expect(screen.getByText(/A.Missing 2.0 or newer/)).toBeInTheDocument();
    expect(screen.getByText("(optional)")).toBeInTheDocument();
    expect(screen.getByText("Top")).toBeInTheDocument();
  });
});
