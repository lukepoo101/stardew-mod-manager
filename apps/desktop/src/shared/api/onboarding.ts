export async function skipOnboarding(): Promise<void> {
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
    return;
  }

  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("set_onboarding_disposition", { disposition: "skipped" });
}
