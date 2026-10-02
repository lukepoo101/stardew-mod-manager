import type {
  ModListItemDto,
  ProfileOverviewDto,
} from "@/shared/api/generated";

/**
 * Public, versioned inventory schema. It is deliberately independent of the
 * internal DTOs so consumers do not depend on database keys or absolute local
 * paths, which are excluded on purpose.
 */
export const INVENTORY_SCHEMA = "stardew-mod-manager.inventory";
export const INVENTORY_SCHEMA_VERSION = 1;

export interface InventoryComponent {
  unique_id: string;
  name: string;
  author: string;
  version: string;
  enabled: boolean;
  /** Why the component is installed, as recorded by the manager. */
  installed_reason: string;
  /** SHA-256 of the retained source package. */
  artifact_hash: string;
  /**
   * Where the package came from. Only local installs exist today; the field is
   * explicit so an unknown source is never mistaken for a verified one.
   */
  source: { kind: "local" | "unknown" };
  /**
   * `missing_required` when the manager reported a required dependency of this
   * component as absent, `satisfied` when health was assessed and reported
   * none, `unknown` when no health assessment was available.
   */
  dependency_status: "satisfied" | "missing_required" | "unknown";
  /**
   * Every requirement problem health reported for this component:
   * `missing`, `disabled`, `too_old` or `unassessed`. Empty when none were
   * reported or health was not assessed (see `dependency_status`).
   */
  requirement_problems: Array<
    "missing" | "disabled" | "too_old" | "unassessed"
  >;
  /** The mod's folder is not where the manager put it. */
  folder_missing: boolean;
  /**
   * Whether the folder's files were compared with what was installed for
   * this export. Always false: the export does not hash files, so local
   * changes are unknown here rather than reported as absent.
   */
  files_checked: false;
}

export interface Inventory {
  schema: typeof INVENTORY_SCHEMA;
  schema_version: number;
  manager_version: string | null;
  generated_at: string;
  profile: { name: string; revision: number };
  game: { operating_system: string; storefront: string };
  smapi: { installed: boolean; observed_version: string | null };
  components: InventoryComponent[];
}

const REQUIREMENT_PROBLEM: Record<
  string,
  InventoryComponent["requirement_problems"][number]
> = {
  MISSING_DEPENDENCY: "missing",
  DEPENDENCY_DISABLED: "disabled",
  DEPENDENCY_TOO_OLD: "too_old",
  DEPENDENCY_UNASSESSED: "unassessed",
};

export function buildInventory(
  overview: ProfileOverviewDto,
  mods: ModListItemDto[],
  options: { generatedAt: string; managerVersion?: string | null },
): Inventory {
  // Sorted so repeated exports of an unchanged profile are byte-identical and
  // therefore diff cleanly.
  const assessed = Boolean(overview.health_summary);
  const brokenIds = new Set(
    (overview.health_summary?.findings ?? [])
      .filter((finding) => finding.code === "MISSING_DEPENDENCY")
      .flatMap((finding) => finding.affected_entities),
  );
  const problemsFor = (uniqueId: string) => {
    const problems = new Set<
      InventoryComponent["requirement_problems"][number]
    >();
    for (const finding of overview.health_summary?.findings ?? []) {
      if (!finding.affected_entities.includes(uniqueId)) continue;
      const kind = REQUIREMENT_PROBLEM[finding.code];
      if (kind) problems.add(kind);
    }
    return [...problems].sort();
  };
  const components = mods
    .map<InventoryComponent>((mod) => ({
      unique_id: mod.unique_id,
      name: mod.name,
      author: mod.author,
      version: mod.version,
      enabled: mod.enabled,
      installed_reason: mod.installed_reason,
      artifact_hash: mod.artifact_hash,
      source: { kind: mod.artifact_hash ? "local" : "unknown" },
      dependency_status: !assessed
        ? "unknown"
        : brokenIds.has(mod.unique_id)
          ? "missing_required"
          : "satisfied",
      requirement_problems: problemsFor(mod.unique_id),
      folder_missing: Boolean(mod.folder_missing),
      files_checked: false,
    }))
    .sort(
      (a, b) =>
        a.unique_id.localeCompare(b.unique_id) ||
        a.version.localeCompare(b.version),
    );

  return {
    schema: INVENTORY_SCHEMA,
    schema_version: INVENTORY_SCHEMA_VERSION,
    manager_version: options.managerVersion ?? null,
    generated_at: options.generatedAt,
    profile: {
      name: overview.profile.name,
      revision: overview.profile.revision,
    },
    game: {
      operating_system: overview.game.operating_system,
      storefront: overview.game.storefront,
    },
    smapi: {
      installed: overview.smapi_status.is_installed,
      observed_version: overview.smapi_status.observed_version,
    },
    components,
  };
}

export function serializeInventory(inventory: Inventory): string {
  return `${JSON.stringify(inventory, null, 2)}\n`;
}
