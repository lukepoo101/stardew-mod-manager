/**
 * Platform labels are display strings only.
 *
 * The backend reports the platform it actually detected, so the UI never
 * decides whether it is running on Windows - it only renders what the
 * inspection returned. These helpers keep the vocabulary identical across
 * onboarding, settings and diagnostics.
 */

const OPERATING_SYSTEM_LABELS: Record<string, string> = {
  windows: "Windows",
  linux: "Linux",
  macos: "macOS",
};

const STOREFRONT_LABELS: Record<string, string> = {
  steam: "Steam",
  gog: "GOG",
  manual: "Manual Folder",
  unknown: "Unknown",
};

export function operatingSystemLabel(value: string | null | undefined): string {
  if (!value) return "Unknown platform";
  return OPERATING_SYSTEM_LABELS[value.toLowerCase()] ?? value;
}

export function storefrontLabel(value: string | null | undefined): string {
  if (!value) return "Unknown";
  return STOREFRONT_LABELS[value.toLowerCase()] ?? value;
}

/**
 * A short "Steam on Windows" style description for a discovered installation.
 *
 * The platform is part of the label because the same Steam library name means
 * different things on different hosts, and a Proton prefix is worth seeing.
 */
export function installationLabel(
  storefront: string | null | undefined,
  operatingSystem: string | null | undefined,
): string {
  const store = storefrontLabel(storefront);
  if (!operatingSystem) return store;
  return `${store} · ${operatingSystemLabel(operatingSystem)}`;
}
