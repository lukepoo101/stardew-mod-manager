# Recipe format

A recipe describes a profile's mods without any local paths. Plain recipe
exports, published collection revisions and the `recipe.json` inside a bundle
all use it. It is JSON with this header:

```json
{
  "schema": "stardew-mod-manager.profile-recipe",
  "schema_version": 1,
  "generated_at": "2026-10-03T12:00:00Z",
  "profile_name": "Cozy Valley",
  "game": { "storefront": "steam", "smapi_version": "4.1.10" },
  "components": []
}
```

A file with a different `schema` is refused as not being a recipe. A different
`schema_version` is refused with an error that names both versions. Both
parsers (Rust in `manager-core::recipe` and TypeScript in
`shared/recipe/recipe.ts`) reject wrong types instead of guessing. Fields added
within version 1 are optional, so older recipes keep reading.

## Components

| Field | Meaning |
| --- | --- |
| `unique_id`, `name`, `author`, `version`, `enabled` | The mod as installed |
| `artifact_hash` | SHA-256 of the package it came from, or empty if unknown |
| `optional` | Recipients may decline it |
| `version_rule` | `exact` (default) or `at_least` |
| `group` | The option group it belongs to |
| `manual` | `{ url, instructions }` for a file the recipient fetches by hand |
| `client_only` | Only matters on each player's own computer |
| `note` | The curator's reason for it, shown as their words |
| `settings` | `[{ path, sha256, content }]`: shared `config.json` files as text |
| `update_keys` | The manifest's UpdateKeys (such as `Nexus:1915`), as declared |
| `source_url` | A page the exporter added for the mod themselves |
| `requires` | UniqueIDs its manifest requires |
| `locally_modified` | Its files differed from its package when exported (bundles) |

Settings paths must be plain relative paths to a `config.json` of at most 256
KB. Their checksum is SHA-256 of the text with CRLF read as LF, so the same
settings compare equal on every operating system. The backend refuses an
entry whose checksum does not match its text. Values are never compared by eye
or shown unless the recipient applies them.

## Recipe-level fields

| Field | Meaning |
| --- | --- |
| `collection` | `{ id, name, author, revision, notes, forked_from? }` for a published collection revision |
| `groups` | `[{ name, description, choose: "any" \| "one" }]` option groups |
| `frozen` | `{ frozen_at, reason }` when the profile was frozen as exported |
| `incomplete` | `[{ unique_id, name, version, artifact_hash }]`: required mods the exporter's own reference asked for but which were not installed |

## Never included

Absolute paths, credentials, saves, operation history and anything a mod writes
outside its `config.json`. Packages travel only inside a bundle, as
`packages/<sha256>.zip`. See `operations-and-recovery.md`.
