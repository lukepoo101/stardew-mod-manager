import React from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import {
  QueryClient,
  QueryClientProvider,
  useQuery,
} from "@tanstack/react-query";

describe("server state cache", () => {
  it("refreshes mounted queries after a mutation invalidates their data", async () => {
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    let serverValue = "Main";
    const queryFn = vi.fn(async () => serverValue);
    const wrapper = ({ children }: { children: React.ReactNode }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(
      () =>
        useQuery({ queryKey: ["profile-overview"], queryFn, staleTime: 60000 }),
      { wrapper },
    );
    await waitFor(() => expect(result.current.data).toBe("Main"));
    serverValue = "Seasonal";
    act(() => client.invalidateQueries({ queryKey: ["profile-overview"] }));
    await waitFor(() => expect(result.current.data).toBe("Seasonal"));
    expect(queryFn).toHaveBeenCalledTimes(2);
  });

  it("matches whole query-key elements when invalidating", () => {
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    client.setQueryData(["mods", "a"], 1);
    client.setQueryData(["mods", "ab"], 2);
    client.invalidateQueries({ queryKey: ["mods", "a"] });
    expect(client.getQueryState(["mods", "a"])?.isInvalidated).toBe(true);
    expect(client.getQueryState(["mods", "ab"])?.isInvalidated).toBe(false);
  });
});
