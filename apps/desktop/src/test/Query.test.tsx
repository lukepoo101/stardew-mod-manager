import React from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { QueryClient, QueryClientProvider, useQuery } from "@/shared/api/query";

describe("server state cache", () => {
  it("refreshes mounted queries after a mutation invalidates their data", async () => {
    const client = new QueryClient();
    let serverValue = "Main";
    const queryFn = vi.fn(async () => serverValue);
    const wrapper = ({ children }: { children: React.ReactNode }) => <QueryClientProvider client={client}>{children}</QueryClientProvider>;
    const { result } = renderHook(() => useQuery({ queryKey: ["profile-overview"], queryFn, staleTime: 60000 }), { wrapper });
    await waitFor(() => expect(result.current.data).toBe("Main"));
    serverValue = "Seasonal";
    act(() => client.invalidateQueries({ queryKey: ["profile-overview"] }));
    await waitFor(() => expect(result.current.data).toBe("Seasonal"));
    expect(queryFn).toHaveBeenCalledTimes(2);
  });

  it("deduplicates shared queries and refreshes invalidations during an in-flight request", async () => {
    const client = new QueryClient();
    let finish!: (value: string) => void;
    const queryFn = vi.fn().mockImplementationOnce(() => new Promise<string>(resolve => { finish = resolve; })).mockResolvedValue("updated");
    const wrapper = ({ children }: { children: React.ReactNode }) => <QueryClientProvider client={client}>{children}</QueryClientProvider>;
    const { result } = renderHook(() => [useQuery({ queryKey: ["mods"], queryFn }), useQuery({ queryKey: ["mods"], queryFn })], { wrapper });
    await waitFor(() => expect(queryFn).toHaveBeenCalledTimes(1));
    act(() => client.invalidateQueries({ queryKey: ["mods"] }));
    await act(async () => finish("outdated"));
    await waitFor(() => expect(result.current.map(query => query.data)).toEqual(["updated", "updated"]));
    expect(queryFn).toHaveBeenCalledTimes(2);
  });

  it("matches whole query-key elements when invalidating", () => {
    const client = new QueryClient();
    client.setQueryData(["mods", "a"], 1);
    client.setQueryData(["mods", "ab"], 2);
    client.invalidateQueries({ queryKey: ["mods", "a"] });
    expect(client.getEntry(["mods", "a"])?.updatedAt).toBe(0);
    expect(client.getEntry(["mods", "ab"])?.updatedAt).toBeGreaterThan(0);
  });
});
