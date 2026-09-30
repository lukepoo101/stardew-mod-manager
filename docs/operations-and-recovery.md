# Stardew Mod Manager - Operations & Recovery

This is the operational contract of the durable operation engine: what is
persisted, what each state means, how restart recovery decides, and what an error
promises. It describes current behaviour, not a migration plan.

## Plan schema versions

`operations.plan_schema_version` is the compatibility switch, not a database
schema version.

| Version | Written by | Recovery |
| --- | --- | --- |
| `0`/historical | the pre-architecture runtime, reconciled by migration 0007 | migrated historical-operation reconciliation |
| `1` (`OPERATION_PLAN_SCHEMA_V1`) | builds before the durable step engine | conservative plan-v1 compatibility reconciliation |
| `2` (`OPERATION_PLAN_SCHEMA_V2`) | this build | deterministic step-based engine |

Only new operations use v2. Existing v1 rows are never rewritten, and an
unsupported plan schema fails safely without mutating anything.

## Persisted steps

`operation_steps` records the boundaries an operation crossed. For v2:

```text
install:  1 retain_artifact          2 inspect_and_stage      3 verify_staged
          4 publish_deployment        5 commit_install_database 6 cleanup_staging
          7 quarantine_published_deployment   (compensation, only when needed)

removal:  1 quarantine_deployment     2 commit_removal_database
          3 restore_quarantined_deployment    (compensation, only when needed)

SMAPI:    1 download_smapi_installer  2 install_smapi_files    3 persist_smapi_state
```

Each live side effect is bracketed: the step is persisted `Running` before the
effect is attempted, and `Completed` only once evidence confirms it happened. A
compensation step is persisted only when compensation actually becomes necessary
- there are no pre-created rollback steps pretending to have succeeded. Step
payloads carry only trusted, ID-derived evidence (artifact hash, profile id,
relative deployment folder, expected revision, observed version); never an
arbitrary absolute managed path.

## State lifecycle

```text
Draft -> Prepared -> Running -> Committing -> Succeeded
                 \          \            \
                  \          -> Failed    -> RecoveryRequired
                   -> Cancelled
```

- `Draft`: inspection and staging in progress; nothing live has happened. Safe to
  cancel and clean up.
- `Prepared`: the semantic plan is frozen. `plan_json`,
  `plan_schema_version`, the operation kind and `expected_profile_revision` are
  never rewritten from here on.
- `Running` / `Committing`: live filesystem moves and database transitions are
  underway.
- `Succeeded`: written only when both filesystem and database state are
  consistent.
- `Failed`: the operation ended without a live side effect that needs attention.
- `RecoveryRequired`: automated recovery could not prove a safe terminal state.

Every runtime transition is validated against the state machine and written
through one lifecycle helper. The single exception is the atomic
`Committing -> Succeeded` inside an install/removal commit, which happens in the
same SQLite transaction as the semantic mutation and asserts its expected
previous state. The execution step that names that commit is completed *in that
same transaction*: the journal and the domain records are one persistence
boundary, so a crash can never leave a durable install described by a step that
still says "running", and a failed commit can never leave a step that says
"completed".

Housekeeping steps are not authoritative, but they still report the truth:
staging cleanup runs after a durably committed install and never fails the
operation, and a cleanup that fails is recorded as failed rather than as
completed.

No authoritative transition write is discarded: if persisting
`RecoveryRequired` itself fails, the returned error says the state is
untrustworthy, asks for manual intervention, and carries the operation id and
the original failure.

## Resource locking

Three mechanisms, three questions:

- `operation_resources` (persisted) - which unresolved durable work owns this
  resource. This survives a restart, so a historical `RecoveryRequired`
  operation still blocks a conflicting new write after the application restarts.
- The in-process resource coordinator - what is executing in this process now.
  Read + Read coexist; anything involving a Write conflicts on the same resource
  identity; disjoint profiles or game installations proceed independently. A
  conflicting request fails fast with `RESOURCE_BUSY` (retryable). There is no
  queue.
- `FileInstanceLock` - cross-process exclusion. It is re-entrant within one
  process, so two disjoint local operations do not look like a second instance to
  each other, while a genuinely different process is still excluded.

Operations acquire their persisted claims before entering live execution or
recovery and hold them for the duration. A launch takes transient `Read` claims
on its profile and game installation; SMAPI setup takes a `Write` claim on the
game installation. Launch and SMAPI participate without inventing operation rows.

## Restart recovery

At startup, unresolved operations are enumerated and reconciled individually. A
single ambiguous operation never stops the application from opening: its evidence
is persisted on the operation and the bootstrap read model keeps reporting it.
Only a failure that prevents recovery processing itself - such as being unable to
read the operations table - is fatal. If another process holds the instance lock,
or the game is running, reconciliation is deferred rather than forced.

Recovery is routed by provenance: migrated historical operations, plan-v1
operations, and plan-v2 operations each use their own reconciler.

For v2 the decision uses the operation state, its persisted steps, live
deployment evidence, recovery-tree evidence, authoritative database ownership and
the profile revision. The proof is always **conjunctive**: database ownership
alone never ends an operation, and neither does filesystem evidence alone.

- Install: this operation's own deployment - identified by the id recorded in its
  publish step, the artifact hash frozen in its plan, and the profile components
  the commit created - must be owned by the database **and** the folder it
  describes must be live, with no quarantined copy in the recovery tree. A
  deployment at the same path that belongs to something else is never proof, and
  a folder another deployment already occupies is never adopted.
- Removal: the exact planned deployment must be marked removed in the database
  **and** its folder must be absent from the live profile. Either half alone
  leaves the operation in `RecoveryRequired`.
- Ambiguity - both copies present, or a folder the database cannot claim - is
  decided before any ownership claim can turn it into success.
- Corrupted persisted plan data, such as a malformed component id in a removal
  plan, fails before anything is mutated rather than committing a partial
  removal. Execution and recovery share one strict decoder, so the same plan is
  refused identically on both paths: execution refuses it during preflight,
  before the operation enters its mutation lifecycle or starts a step, and
  recovery - which cannot simply refuse an operation that may already have
  crossed a live boundary - records it as `RecoveryRequired`.

Deliberate rules:

- publication that never completed is retried when its staged source is still
  valid, and otherwise ends as a terminal failure that asks for a fresh plan;

- a live folder the database does not own is either adopted by the atomic commit
  (when the prepared revision is still current) or compensated by being moved
  into the recovery tree;
- a quarantined deployment whose database commit never landed is committed;
- a database commit that already happened completes the operation;
- ambiguous evidence - both copies present, or a deployment missing while the
  database still owns it - requires manual reconciliation.

### Evidence semantics

`Ok(true)` is present, `Ok(false)` is absent, and `Err` means the system cannot
tell. An unreadable evidence query leaves the operation in `RecoveryRequired`
instead of being treated as absence, because `Failed` would claim more than the
system knows.

## RecoveryRequired semantics

While an operation is `RecoveryRequired`:

- its filesystem evidence - staging, quarantine and recovery folders - is
  preserved;
- the operations that declared a conflicting resource are blocked;
- the errors returned for it are `category = recovery` and
  `recoverability = requires_manual_intervention`, with `operation_id` set to the
  affected operation. The original diagnosis (code, summary, technical details)
  is preserved: only the recovery semantics are promoted.

Recovery is never cleared merely to make the application usable.

## Error and recovery contract

Every product command returns `IpcResult<T>`; a rejected invoke reaches the
frontend as `ApiClientError` carrying code, category, recoverability, summary,
technical details and operation id. Stable codes relevant here include:

| Code | Meaning |
| --- | --- |
| `RESOURCE_BUSY` | another in-process operation holds the resource; retryable |
| `PROFILE_OPERATION_UNRESOLVED` | an unresolved durable operation owns the profile; manual intervention |
| `PREVIEW_STALE` | the profile moved past the prepared revision; retry with a fresh plan |
| `PERSISTED_*_INVALID` | a persisted value could not be decoded; storage error, no mutation |
| `RECOVERY_STATE_PERSIST_FAILED` | the operation needs manual reconciliation and that fact could not be recorded |
| `UNSUPPORTED_OPERATION_PLAN_SCHEMA` | recovery does not know this plan schema; nothing was mutated |

## Historical operation compatibility

Published migrations (including migration 0007), the legacy database upgrade
tests, migrated historical-operation reconciliation and plan-schema-v1
reconciliation remain in place. They exist so databases written by older released
versions still open and are reconciled safely. They never create new operations,
and they are not an alternate runtime architecture.

## Stale-plan protection

Each profile has a monotonically increasing revision. A prepared operation
captures the expected revision, and the authoritative commit compares it inside
the SQLite transaction. If the profile changed after the preview, the commit
fails with an operation conflict and the user must prepare a fresh plan.

## Artifact retention

Newly selected archives are copied into content-addressed manager storage before
install preparation. Installations whose original archive was never retained
migrate with metadata-only placeholder artifact rows, so the installed deployment
stays representable without pretending its source bytes exist.

### Storage cleanup

Settings > Storage cleanup previews and removes manager-owned items only:
retained package archives (`packages/<sha256>.zip`), the SMAPI installer cache,
and each profile's `.staging/<operation>` and `.recovery/<operation>` folders.
The backend builds the plan (`StorageCleanupService`); the frontend can only pick
item ids from it, and running the cleanup plans again and skips any id that is no
longer removable.

| Kept | Why |
| --- | --- |
| A package any profile deploys, archived profiles included | reinstall and rollback may need it |
| Every package and leftover while any operation is unresolved (including a pending preview or `RecoveryRequired`) | the plan or recovery may reference them |
| Staging/recovery folders of a non-terminal operation | the journal may still use them |
| Any link, anywhere in the scanned roots | cleanup never follows or removes links |

Leftovers of terminal operations, and folders whose operation id is not recorded
at all, are removable. The run holds write claims on every game and profile and
the instance lock, so no operation can start while it deletes. Each requested
item gets its own `removed`/`skipped`/`failed` outcome; the result is only
`complete` when all were removed, and repeating a cleanup is harmless. Removing a
package deletes only the archive file: its catalog row stays so history remains
readable, and importing the same file again restores it.

## Enabling and disabling mods

Enabling or disabling is not a durable operation, because it moves one folder
and flips a flag rather than replacing content. It is written to be repeatable
instead: it always reconciles towards the requested state from wherever the
files actually are, so asking again after an interruption heals a half-applied
change.

- The deployment folder is the unit. Components that arrived in one package
  share a folder and change together; the confirmation names them.
- A disabled mod's folder lives in the profile's `disabled` directory, outside
  the mods path SMAPI scans.
- The change is refused while the game is running or while another operation
  owns the profile, and it increments the profile revision.
- Launch preflight blocks when the recorded state and the files disagree, and
  says how to repair it.
- A disabled mod must be enabled before it can be removed.

### Several at once

Selecting mods on the Mods page and choosing *Enable selected* or *Disable
selected* reviews the whole set first (`impact_many`): every mod that moves,
including ones sharing a package with a selected mod; enabled mods outside the
selection that would stop loading; and requirements the set leaves unmet. A
requirement met by another mod in the same set is not reported. It then runs
under one profile lock, moving each package folder once. A failure on one
package does not undo the others; the result lists what changed and what did
not, and because every move is repeatable, running the same request again
finishes the job.

## Profile bundles

A bundle (`*.smm-bundle.zip`) is a recipe (`recipe.json`, the same schema as a
profile recipe) plus the packages it names as `packages/<sha256>.zip`. It moves
a profile between computers or people including the mods themselves.

- **Export** writes a new file (never overwriting one) and only ever exposes a
  finished file: it is written as `.part` and renamed. Mods whose package is no
  longer stored are listed as not carried.
- **Import** always creates a **new** profile; existing profiles are never
  modified. The archive is treated as untrusted: only `recipe.json` and
  `packages/<hash>.zip` entries are accepted, sizes are bounded, and every
  package is verified against the digest in its name before use. A damaged or
  altered bundle is refused before a profile is created.
- Mods are installed through the normal install engine, retrying what is left
  until a pass installs nothing new, so dependencies do not need to be listed in
  any order. Anything that cannot be installed is reported per mod with a reason.
- Mods disabled in the source are installed and then left disabled.
- Bundles carry third-party code, so the UI warns to import only from people the
  user trusts (see `docs/trust-model.md`).

## Guided fault isolation

"Find the mod causing a problem" (Diagnostics) bisects the enabled mods. The
original enabled state of every component is stored in the preferences table
before the first mod is turned off, and "Restore my original mods" re-applies
exactly that, so a session can be abandoned at any step.

- Units are deployment folders, and a unit's required dependencies always come
  with it, so a dependency is never separated from what needs it.
- Only mods that were on can be suspects; mods the user turned off stay off.
- Step 1 turns everything off. If the problem persists the session reports that
  it is probably not a mod. Otherwise each step enables half of the suspects
  (plus what they need and what has been cleared) and asks whether the problem
  happens.
- It assumes one cause. A problem that needs two mods together ends as
  "inconclusive" rather than blaming either.
- The pure algorithm lives in `manager-core::troubleshoot`; applying it uses the
  same repeatable enable/disable operation as the Mods page.

## Deleting a profile

Only **archived** profiles can be deleted, so the active profile is never the
target; archiving first is the explicit decision about what stays active. The
dialog lists what is removed (the profile, its installed mods and their
settings, with the folder size) and what is kept (downloaded packages, other
profiles, SMAPI, the game, saves and Activity history), and needs the profile's
name typed.

`ProfileDeletionService::delete` refuses while a change to the profile is
unresolved or the game is running, then, under the profile's write claim and
the instance lock:

1. moves `setups/<profile>` to `trash/profile-<id>-<timestamp>` with a rename
   (atomic within the data folder, so nothing is half copied);
2. deletes the profile row in one transaction; its components and deployments
   cascade, and default/last-used/active references to it are cleared;
3. records a `profile_delete` operation so the deletion appears in Activity.

If step 2 fails the folder is renamed back, leaving the profile whole. Packages
are never deleted here; storage cleanup removes the ones nothing uses. Folders
in `trash/` can be restored by hand until they are removed.

## Freezing a profile

Profiles → *Freeze this profile* records the current mods (UniqueID, name,
version, package checksum, enabled state) with a time and optional reason, in
the preferences table under `frozen_profiles`. While frozen:

- new install/removal previews are refused (`PROFILE_FROZEN`), and so is
  committing one prepared before the freeze;
- enabling and disabling stay allowed (fault isolation depends on them) and are
  shown as drift from the snapshot, alongside anything added or missing;
- a *Frozen* badge sits next to the profile name in the header.

Unfreezing only removes the freeze; it applies nothing.

## Duplicating a profile

*Duplicate* on a profile card creates an independent copy: a new profile id and
folder, the same mods and versions installed from the retained packages through
the normal install engine (the same path as importing a bundle), and the same
enabled state. The copy's description records what it was copied from. The
source is never modified and stays active. Mods whose package is no longer
stored are listed as not copied.

## Experiments

*Profiles → Try changes safely → Start an experiment* duplicates the active
profile (as above), marks the copy as an experiment of its source (source id,
name and revision, in the preferences table under `profile_experiments`) and
activates the copy. The source is never touched. While an experiment is active
the card offers:

- **Keep as a normal profile**: removes the mark; nothing else changes.
- **Discard**: activates the source again, then archives and deletes the
  experiment through the ordinary profile deletion path (its folder goes to the
  trash), and removes the mark.

Nothing made in an experiment is ever copied back automatically.

## Saves

The Profiles page lists Stardew Valley saves from the game's save folder
(`~/.config/StardewValley/Saves` or `%APPDATA%\StardewValley\Saves`), reading
only each save's `SaveGameInfo`. A save is identified by its folder name, which
the game never renames.

- **Usual profile**: a link from a save to a profile id, stored in the
  preferences table (`save_associations`). Renaming the profile keeps the link;
  a link to a deleted profile is shown as such. Linking never touches the save.
- **Mismatch warning**: when the most recently played save is linked to a
  profile other than the active one, the card says so. It is based on which
  save was played last, not on which one will be loaded, and says that.
- **Backups** are copied to `save-backups/<save>/<timestamp>` in the manager's
  data folder (as `.part` first), checked file by file against the original,
  and only then renamed into place. **Restore** first backs up the live save
  (`…-before-restore`), copies the backup next to it and checks it, and only
  then replaces the live save. Both are refused while the game is running.
- Save backups and restores are separate from mod and profile recovery: neither
  changes installed mods, and profile changes never touch saves.

## Last known good

When a modded session is confirmed (SMAPI's log shows the profile's mods
loaded; starting the process is not enough), the launch service records the
profile's mods (UniqueID, version, package checksum, enabled state) with the
game and SMAPI versions, in the preferences table under `known_good:<profile>`.
The Profiles page compares the profile with that record. **Restore enabled
state** switches mods back through the reviewed bulk enable/disable path and
turns off mods installed since; version changes and removed mods are listed for
the user to reinstall, because a restore never installs or removes anything.

## Checking mod files

*Diagnostics → Check mod files* compares each deployed mod folder (live or
disabled) with the file inventory its install recorded: every staged file's
path, size and SHA-256. It reports files that are **missing**, **changed**
(contents differ), **settings edited** (`config.json`, which is normal to
change) and **added since install** (often created by the mod itself). Folders
installed before inventories were recorded show *no install record* instead of
a guess. Checking only reads; it never repairs or reinstalls.

## Group reference

*Profiles → Group reference* keeps a shared recipe (for example a multiplayer
group's setup) as the active profile's reference, in the preferences table
under `reference_recipe:<profile>`. It is validated with the same strict recipe
parser before it is kept. The profile is compared with it by UniqueID, version,
package checksum and enabled state whenever the page is open. Individual
differences can be **accepted** for the group; an accepted difference is keyed
by both versions, so it returns if either side changes. Replacing the reference
clears accepted differences. Nothing is ever installed or changed.
