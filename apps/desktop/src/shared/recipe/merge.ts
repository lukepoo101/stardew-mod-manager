import type { ModListItemDto } from "@/shared/api/generated";
import type { ProfileRecipe, RecipeComponent } from "./recipe";

/**
 * Updating a customised profile to a newer collection revision, worked out
 * from three sides: the revision it followed (base), the new revision
 * (theirs) and the profile as it is (mine). Nothing here changes anything.
 *
 * - "upstream": the collection changed and you had not: take it.
 * - "conflict": both changed, differently: you choose.
 * - "local": only you changed it: your customisation is kept.
 */
export type MergeKind = "upstream" | "conflict" | "local";

export interface MergeItem {
  unique_id: string;
  name: string;
  kind: MergeKind;
  base?: RecipeComponent;
  theirs?: RecipeComponent;
  mine?: ModListItemDto;
  /** What the collection's change is, in words. */
  upstream: string;
  /** What your change is, in words. */
  local: string;
}

type Side = { version: string; hash: string; enabled: boolean } | null;

const fromRecipe = (c?: RecipeComponent): Side =>
  c
    ? {
        version: c.version,
        hash: c.artifact_hash.toLowerCase(),
        enabled: c.enabled,
      }
    : null;
const fromMod = (m?: ModListItemDto): Side =>
  m
    ? {
        version: m.version,
        hash: m.artifact_hash.toLowerCase(),
        enabled: m.enabled,
      }
    : null;

function same(a: Side, b: Side): boolean {
  if (!a || !b) return a === b;
  return (
    a.version === b.version &&
    (!a.hash || !b.hash || a.hash === b.hash) &&
    a.enabled === b.enabled
  );
}

function describe(from: Side, to: Side): string {
  if (!from && to) return `added (${to.version})`;
  if (from && !to) return "removed";
  if (!from || !to) return "unchanged";
  if (from.version !== to.version) return `${from.version} → ${to.version}`;
  if (from.hash && to.hash && from.hash !== to.hash)
    return `same version, different file`;
  if (from.enabled !== to.enabled)
    return to.enabled ? "turned on" : "turned off";
  return "unchanged";
}

export function planMerge(
  base: ProfileRecipe,
  theirs: ProfileRecipe,
  mine: readonly ModListItemDto[],
): MergeItem[] {
  const key = (id: string) => id.toLowerCase();
  const b = new Map(base.components.map((c) => [key(c.unique_id), c]));
  const t = new Map(theirs.components.map((c) => [key(c.unique_id), c]));
  const m = new Map(mine.map((x) => [key(x.unique_id), x]));
  const ids = new Set([...b.keys(), ...t.keys(), ...m.keys()]);
  const items: MergeItem[] = [];
  for (const id of ids) {
    const [bc, tc, mm] = [b.get(id), t.get(id), m.get(id)];
    // An optional mod you do not have is a recommendation, not a change.
    if (tc?.optional && !mm && !bc) continue;
    const [bs, ts, ms] = [fromRecipe(bc), fromRecipe(tc), fromMod(mm)];
    const upstreamChanged = !same(bs, ts);
    const localChanged = !same(ms, bs);
    let kind: MergeKind | null = null;
    if (upstreamChanged && !localChanged) kind = "upstream";
    else if (upstreamChanged && localChanged && !same(ms, ts))
      kind = "conflict";
    else if (!upstreamChanged && localChanged) kind = "local";
    if (!kind) continue;
    items.push({
      unique_id: tc?.unique_id ?? bc?.unique_id ?? mm?.unique_id ?? id,
      name: tc?.name || bc?.name || mm?.name || id,
      kind,
      base: bc,
      theirs: tc,
      mine: mm,
      upstream: describe(bs, ts),
      local: describe(bs, ms),
    });
  }
  const order: MergeKind[] = ["conflict", "upstream", "local"];
  return items.sort(
    (x, y) =>
      order.indexOf(x.kind) - order.indexOf(y.kind) ||
      x.name.localeCompare(y.name),
  );
}
