# Profile inventory export (schema v1)

`Export inventory` on the Mods page writes `profile-inventory.json`. It is a
public, versioned contract, independent of internal database rows.

```json
{
  "schema": "stardew-mod-manager.inventory",
  "schema_version": 1,
  "manager_version": "0.1.0",
  "generated_at": "2026-09-29T10:00:00.000Z",
  "profile": { "name": "Default", "revision": 3 },
  "game": { "operating_system": "Linux", "storefront": "Steam" },
  "smapi": { "installed": true, "observed_version": "4.1.10" },
  "components": [
    {
      "unique_id": "Pathoschild.ContentPatcher",
      "name": "Content Patcher",
      "author": "Pathoschild",
      "version": "2.0.0",
      "enabled": true,
      "installed_reason": "explicit",
      "artifact_hash": "sha256...",
      "source": { "kind": "local" },
      "dependency_status": "satisfied",
      "requirement_problems": [],
      "folder_missing": false,
      "files_checked": false
    }
  ]
}
```

- Components are sorted by `unique_id` then `version`, so an unchanged profile
  exports identically apart from `generated_at`.
- `source.kind` is `local` when the manager holds the package it installed from
  and `unknown` otherwise. Unknown is never reported as verified.
- Absolute paths, credentials, database identifiers and notes are never
  included.
- Consumers must check `schema` and reject a `schema_version` they do not know.
  New optional fields may be added within a version; removals or meaning
  changes bump it.

`dependency_status` is `missing_required` when the manager's health check
reported one of the component's required dependencies as absent, `satisfied`
when health was assessed and reported none, and `unknown` when no assessment was
available. Optional integrations never affect it.

Added within v1 (optional for consumers):

- `requirement_problems` lists every requirement problem health reported for
  the component: `missing`, `disabled`, `too_old` or `unassessed`.
- `folder_missing` is true when the mod's folder is not where the manager put
  it.
- `files_checked` is always `false`: the export does not hash files, so local
  changes are unknown in it, not absent. Use Diagnostics → Check mod files for
  that.

# Support export

`Share for support` on the Diagnostics page builds one plan from allowlisted
sections (environment, profile, mods, findings, last 60 log lines). Every
section is redacted once (home-directory prefixes become `~`, tokens,
authorization headers, credentialed and signed URLs are replaced), so the
preview, the copied summary and the saved bundle are identical in content.
Redaction is best effort: the export re-scans its own output and lists anything
suspicious under "review before sharing", and evidence that could not be
gathered is labelled unavailable rather than omitted as healthy. Nothing is
uploaded.
