import { invokeApi, isTauri } from "./invoke";

export async function skipOnboarding(): Promise<void> {
  if (!isTauri()) {
    return;
  }

  await invokeApi<void>("set_onboarding_disposition", {
    disposition: "skipped",
  });
}
