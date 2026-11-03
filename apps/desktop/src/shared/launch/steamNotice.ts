/** Whether the installation came from Steam, the only store this is about. */
export function isSteam(storefront: string | null | undefined): boolean {
  return storefront?.toLowerCase() === "steam";
}

/**
 * The manager starts the game itself so it can pass the profile's Mods
 * folder. Steam's overlay is injected only into games Steam starts. The game
 * still gets Steam's app id, so Steam usually records playtime, but that is
 * not guaranteed, so it is not promised either way.
 */
export const STEAM_NOTICE =
  "Both start the game directly rather than through Steam, so Steam's overlay is not available in these sessions, and Steam may not count the time played.";
