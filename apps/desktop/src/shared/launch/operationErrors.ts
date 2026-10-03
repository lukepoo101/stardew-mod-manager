import type { LaunchSessionDto } from "@/shared/api/generated";

/**
 * The modded game sessions of a profile just before and just after an
 * operation, for seeing which errors first appeared after it. Either can be
 * missing: then nothing is compared.
 */
export function sessionsAround(
  sessions: readonly LaunchSessionDto[],
  profileId: string,
  at: string,
): { before: LaunchSessionDto | null; after: LaunchSessionDto | null } {
  const when = Date.parse(at);
  const modded = sessions
    .filter((s) => s.profile_id === profileId && s.launch_mode !== "vanilla")
    .sort((a, b) => Date.parse(a.launched_at) - Date.parse(b.launched_at));
  const before = modded.filter((s) => Date.parse(s.launched_at) < when).at(-1);
  const after = modded.find((s) => Date.parse(s.launched_at) > when);
  return { before: before ?? null, after: after ?? null };
}
