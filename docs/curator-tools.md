# Sharing a profile: checks and changelogs

The Profiles page has a "Before you share this profile" card. It works on a
recipe built from the active profile and never installs, removes or publishes
anything.

## Checks

| Check | Fails when | Warns when |
| --- | --- | --- |
| Lists at least one mod | the recipe is empty | |
| Each mod is listed once | a UniqueID appears twice (case-insensitive) | |
| Every mod has an exact version | a version is blank | |
| Pinned to a package checksum | | a mod has no SHA-256 checksum |
| At least one mod is enabled | | every mod is disabled |
| Records the SMAPI version | | none is recorded |
| Names and authors present | | a name or author is blank |
| Says which mods are optional | | nothing is marked optional |

A recipe is "ready to share" when nothing fails; warnings are advice. A blank
author includes a manifest with no `Author` field, which is read as "Unknown".
The enabled check is left out for an empty list.

On a profile some checks are a backstop rather than something a curator will
meet: the installer already refuses a manifest with a blank version and
treats a UniqueID that differs only in case as the same mod, and every
installed mod has a package checksum. They still apply to recipes made any
other way.

**Reproducibility** is the share of mods that have both an exact version and a
package checksum, and do not accept a newer version instead. It measures how precisely a recipient can reproduce the
profile. It is not a quality score, and no check says a mod is safe or will work
for the recipient (see `docs/trust-model.md`).

## Changelog between revisions

Choosing an earlier recipe compares it with the current profile and lists mods
that were added, removed, updated (numeric-aware version order), rolled back,
repackaged (same version, different package checksum) or had their enabled
state changed, plus how many are unchanged. A mod that changed in more than one
way (updated and turned off, say) is listed under each. Mods are matched by
UniqueID regardless of case. The text can be copied into release
notes. Nothing is stored: the earlier recipe is only read.
