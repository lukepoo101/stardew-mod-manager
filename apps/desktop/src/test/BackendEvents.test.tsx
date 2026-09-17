import React from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  QueryClient,
  QueryClientProvider,
  useQuery,
} from "@tanstack/react-query";
import {
  BACKEND_STATE_CHANGED,
  useBackendInvalidationBridge,
} from "@/shared/api/events";
import { useActivateProfile } from "@/shared/api/hooks";

// The Tauri event module is mocked at the module boundary so the bridge is
// exercised exactly as production code uses it: through a dynamic import of
// "@tauri-apps/api/event".
const tauriEvents = vi.hoisted(() => {
  const listeners = new Map<string, (event: unknown) => void>();
  const unlisten = vi.fn();
  const listen = vi.fn(
    async (name: string, handler: (event: unknown) => void) => {
      listeners.set(name, handler);
      return unlisten;
    },
  );
  return { listeners, unlisten, listen };
});

vi.mock("@tauri-apps/api/event", () => ({ listen: tauriEvents.listen }));

vi.mock("@/shared/api/client", () => ({
  api: {
    activateProfile: vi.fn(async () => undefined),
  },
}));

function makeClient() {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } });
}

function wrapperFor(client: QueryClient) {
  return ({ children }: { children: React.ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
}

function enterTauri() {
  (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
}

function leaveTauri() {
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
}

async function listenForBackendStateChange() {
  await waitFor(() =>
    expect(tauriEvents.listeners.has(BACKEND_STATE_CHANGED)).toBe(true),
  );
}

beforeEach(() => {
  tauriEvents.listeners.clear();
  tauriEvents.listen.mockClear();
  tauriEvents.unlisten.mockClear();
  leaveTauri();
});

afterEach(() => {
  leaveTauri();
});

describe("backend invalidation bridge", () => {
  it("invalidates an already-cached query when the backend emits one event", async () => {
    enterTauri();
    const client = makeClient();
    const queryFn = vi.fn(async () => "first");
    const { result } = renderHook(
      () => ({
        query: useQuery({
          queryKey: ["bootstrap"],
          queryFn,
          staleTime: 60000,
        }),
        bridge: useBackendInvalidationBridge(),
      }),
      { wrapper: wrapperFor(client) },
    );

    await waitFor(() => expect(result.current.query.data).toBe("first"));
    await listenForBackendStateChange();
    expect(tauriEvents.listen).toHaveBeenCalledWith(
      BACKEND_STATE_CHANGED,
      expect.any(Function),
    );

    queryFn.mockResolvedValue("second");
    await act(async () => {
      tauriEvents.listeners.get(BACKEND_STATE_CHANGED)?.({});
    });

    await waitFor(() => expect(result.current.query.data).toBe("second"));
    expect(queryFn).toHaveBeenCalledTimes(2);
  });

  it("removes its listener when the bridge unmounts", async () => {
    enterTauri();
    const client = makeClient();
    const { unmount } = renderHook(() => useBackendInvalidationBridge(), {
      wrapper: wrapperFor(client),
    });

    await listenForBackendStateChange();
    expect(tauriEvents.unlisten).not.toHaveBeenCalled();

    unmount();
    await waitFor(() => expect(tauriEvents.unlisten).toHaveBeenCalledTimes(1));
  });

  it("does nothing outside the Tauri runtime", async () => {
    const client = makeClient();
    const { unmount } = renderHook(() => useBackendInvalidationBridge(), {
      wrapper: wrapperFor(client),
    });

    // Give the async subscription a chance to run; nothing may be registered.
    await act(async () => {
      await Promise.resolve();
    });
    expect(tauriEvents.listen).not.toHaveBeenCalled();
    expect(tauriEvents.listeners.size).toBe(0);
    unmount();
  });

  it("leaves mutation hooks free of query-key bookkeeping", async () => {
    enterTauri();
    const client = makeClient();
    const queryFn = vi.fn(async () => "profiles");
    const { result } = renderHook(
      () => ({
        mutation: useActivateProfile(),
        query: useQuery({
          queryKey: ["profiles"],
          queryFn,
          staleTime: 60000,
        }),
        bridge: useBackendInvalidationBridge(),
      }),
      { wrapper: wrapperFor(client) },
    );

    await waitFor(() => expect(result.current.query.data).toBe("profiles"));
    await listenForBackendStateChange();
    expect(queryFn).toHaveBeenCalledTimes(1);

    // The mutation owns no invalidation map: on its own it leaves the cached
    // server state exactly where it was.
    await act(async () => {
      await result.current.mutation.mutateAsync("profile-1");
    });
    expect(queryFn).toHaveBeenCalledTimes(1);

    // The backend event, not the hook, is what refreshes server state.
    await act(async () => {
      tauriEvents.listeners.get(BACKEND_STATE_CHANGED)?.({});
    });
    await waitFor(() => expect(queryFn).toHaveBeenCalledTimes(2));
  });
});
