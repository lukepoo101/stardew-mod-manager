/**
 * Central wording for the third-party mod trust boundary, so install, source
 * and help surfaces cannot drift apart. See docs/trust-model.md.
 */
export const MOD_TRUST_SUMMARY =
  "Mods are third-party programs. Once installed they run with your user account's permissions, and neither SMAPI nor this manager sandboxes them.";

export const MOD_TRUST_DETAIL =
  "The manager checks that a package is a well-formed archive, keeps files inside the mod folder and records a checksum so you can tell later if it changed. Those checks do not tell you what the code does. A valid manifest or a matching checksum is not evidence that a mod is harmless. Only install mods from sources you trust and check where a package came from before you install it.";
