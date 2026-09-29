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
