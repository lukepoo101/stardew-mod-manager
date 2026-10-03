import type { DependencyMapEntryDto } from "@/shared/api/generated";

/**
 * A collection's dependency graph, read from the same dependency map the
 * checks use. Shown as text with focus and filtering rather than a drawing,
 * so it stays usable for large collections and with a screen reader.
 */
export interface Edge {
  from: string;
  to: string;
  toName: string;
  /** "required", "optional" or "content_pack_for". */
  kind: string;
  /** "satisfied", "missing", "disabled" or "too_old". */
  status: string;
}

const key = (id: string) => id.toLowerCase();

export function edges(map: readonly DependencyMapEntryDto[]): Edge[] {
  return map.flatMap((entry) =>
    entry.requires.map((r) => ({
      from: entry.unique_id,
      to: r.unique_id,
      toName: r.name ?? r.unique_id,
      kind: r.kind,
      status: r.status,
    })),
  );
}

/** Cycles through hard requirements, each as the UniqueIDs around it. */
export function cycles(map: readonly DependencyMapEntryDto[]): string[][] {
  const hard = new Map<string, string[]>();
  for (const e of edges(map))
    if (e.kind !== "optional")
      hard.set(key(e.from), [...(hard.get(key(e.from)) ?? []), key(e.to)]);
  const names = new Map(map.map((m) => [key(m.unique_id), m.unique_id]));
  const found = new Set<string>();
  const out: string[][] = [];
  const visit = (node: string, stack: string[]) => {
    const at = stack.indexOf(node);
    if (at >= 0) {
      const cycle = stack.slice(at);
      const sig = [...cycle].sort().join(">");
      if (!found.has(sig)) {
        found.add(sig);
        out.push(cycle.map((id) => names.get(id) ?? id));
      }
      return;
    }
    if (stack.length > 200) return;
    for (const next of hard.get(node) ?? []) visit(next, [...stack, node]);
  };
  for (const node of hard.keys()) visit(node, []);
  return out;
}

/** Everything a mod needs (transitively) and everything that needs it. */
export function neighbourhood(
  map: readonly DependencyMapEntryDto[],
  focus: string,
): Set<string> {
  const all = edges(map);
  const out = new Set([key(focus)]);
  const walk = (start: string, forward: boolean) => {
    const queue = [key(start)];
    while (queue.length > 0) {
      const node = queue.shift() as string;
      for (const e of all) {
        const [a, b] = forward
          ? [key(e.from), key(e.to)]
          : [key(e.to), key(e.from)];
        if (a === node && !out.has(b)) {
          out.add(b);
          queue.push(b);
        }
      }
    }
  };
  walk(focus, true);
  walk(focus, false);
  return out;
}

/**
 * What declining some mods would do: the mods that hard-require one of
 * them, followed through the chain.
 */
export function brokenWithout(
  map: readonly DependencyMapEntryDto[],
  declined: ReadonlySet<string>,
): string[] {
  const gone = new Set([...declined].map(key));
  let changed = true;
  while (changed) {
    changed = false;
    for (const e of edges(map))
      if (
        e.kind !== "optional" &&
        gone.has(key(e.to)) &&
        !gone.has(key(e.from))
      ) {
        gone.add(key(e.from));
        changed = true;
      }
  }
  return map
    .filter(
      (m) => gone.has(key(m.unique_id)) && !declined.has(key(m.unique_id)),
    )
    .map((m) => m.name);
}
