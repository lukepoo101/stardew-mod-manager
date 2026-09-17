import { useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { isTauri } from "./invoke";

/**
 * The one backend cache-invalidation event.
 *
 * The backend emits it without a payload whenever a command may have changed
 * authoritative state. Its whole meaning is "invalidate active backend queries";
 * it deliberately carries no query keys, because maintaining a Rust-to-React
 * dependency graph would be a second source of truth for the same cache.
 */
export const BACKEND_STATE_CHANGED = "backend-state-changed";

/**
 * Subscribes the TanStack Query cache to the backend invalidation event.
 *
 * This is the only production frontend module allowed to import the Tauri event
 * API, exactly as `shared/api/invoke.ts` is the only module allowed to import
 * the Tauri core API. Feature code keeps using query hooks; it never listens for
 * backend events itself.
 */
export function useBackendInvalidationBridge(): void {
  const queryClient = useQueryClient();

  useEffect(() => {
    // Outside Tauri there is no backend to notify, and the browser fallback
    // client serves synthetic data, so the bridge stays inert.
    if (!isTauri()) {
      return;
    }

    let unlisten: (() => void) | undefined;
    let disposed = false;

    void (async () => {
      try {
        const { listen } = await import("@tauri-apps/api/event");
        const stop = await listen(BACKEND_STATE_CHANGED, () => {
          void queryClient.invalidateQueries();
        });
        // The component may have unmounted while the module was loading; the
        // listener must not outlive it.
        if (disposed) {
          stop();
        } else {
          unlisten = stop;
        }
      } catch (error) {
        console.error("failed to subscribe to backend state changes", error);
      }
    })();

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [queryClient]);
}

/** Renders nothing; exists so the bridge can be mounted exactly once. */
export const BackendInvalidationBridge = (): null => {
  useBackendInvalidationBridge();
  return null;
};
