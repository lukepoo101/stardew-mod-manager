import type {
  DownloadedPreviewDto,
  ModUpdateDto,
  OperationPreviewDto,
} from "@/shared/api/generated";

/** Where a downloaded update came from: everything but the preview. */
export type DownloadOrigin = Omit<DownloadedPreviewDto, "preview">;

/** "downloaded from the GitHub release v1.0 of owner/name, which …" */
export function sourceSentence({ source }: DownloadOrigin): string {
  if (source.provider === "github") {
    return `downloaded from the GitHub release ${source.tag} of ${source.repo}${
      source.declared_by_mod
        ? ", which the mod's manifest names"
        : ", which the community compatibility list names"
    }. Who published that release is not verified.`;
  }
  return `downloaded from Nexus Mods: ${source.display_name || source.file_name}${
    source.version ? ` ${source.version}` : ""
  } (file ${source.file_id} of mod ${source.mod_id}). Who uploaded it is not verified.`;
}

/** What the download was checked against, after the stored fingerprint. */
export function integritySentence(origin: DownloadOrigin | null): string {
  if (!origin) return "There is no published fingerprint to compare it with.";
  if (origin.source.provider === "github") {
    return origin.checksum_verified
      ? "It matches the fingerprint GitHub publishes for the file."
      : "There is no published fingerprint to compare it with.";
  }
  return origin.checksum_verified
    ? "Nexus Mods recognised its MD5 as this file."
    : "Nexus Mods did not confirm its MD5, so it could not be compared with the file Nexus lists.";
}

/** Where a downloaded update came from, in one line, for a list. */
export function originLine(origin: DownloadOrigin): string {
  const { source, checksum_verified } = origin;
  if (source.provider === "github") {
    return `${source.asset_name} from GitHub release ${source.tag} of ${source.repo} · ${
      checksum_verified
        ? "matches the checksum GitHub publishes"
        : "GitHub publishes no checksum for it"
    }. Who published that release is not verified.`;
  }
  return `${source.file_name} from Nexus Mods (mod ${source.mod_id}) · ${
    checksum_verified
      ? "Nexus recognised its MD5"
      : "Nexus did not confirm its MD5"
  }. Who uploaded it is not verified.`;
}

/**
 * The updates the last check found that an archive under review is: the same
 * mod at the version the check suggested. An archive the user downloaded by
 * hand from the mod page is matched to the update it was downloaded for.
 */
export function matchingUpdates(
  preview: OperationPreviewDto,
  updates: ModUpdateDto[],
): ModUpdateDto[] {
  return updates.filter(
    (update) =>
      update.update_version !== null &&
      preview.detected_components.some(
        (c) =>
          c.unique_id.toLowerCase() === update.unique_id.toLowerCase() &&
          sameVersion(c.version, update.update_version ?? ""),
      ),
  );
}

/** "2.0" and "2.0.0" are one version, as SMAPI compares them. */
export function sameVersion(a: string, b: string): boolean {
  const parts = (v: string) => {
    const [core, ...rest] = v
      .trim()
      .toLowerCase()
      .split(/(?=[-+])/);
    const numbers = core.split(".");
    if (!numbers.every((n) => /^\d+$/.test(n))) return v.trim().toLowerCase();
    while (numbers.length < 3) numbers.push("0");
    while (numbers.length > 3 && numbers[numbers.length - 1] === "0")
      numbers.pop();
    return [numbers.map((n) => String(Number(n))).join("."), ...rest].join("");
  };
  return parts(a) === parts(b);
}
