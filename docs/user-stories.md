# User stories

These stories describe how people use the manager, from a first launch to
curating collections and scripting it. They are written against the current
behaviour (see `operations-and-recovery.md`, `frontend-ux.md`, `cli.md` and
`curator-tools.md`), so the acceptance criteria double as a checklist for
reviews and release acceptance. Where a story touches a deliberate limit, the
limit is part of the criteria rather than left out.

Stories are grouped by level. Each level assumes the ones before it.

## Personas

| Persona | Who they are |
| --- | --- |
| **New player** | Has never modded Stardew Valley. Wants a few mods working without reading guides. |
| **Regular modder** | Installs and swaps mods often, keeps several playthroughs going. |
| **Multiplayer host** | Runs a farm with friends and needs everyone on the same mod set. |
| **Troubleshooter** | Something broke after a change and they want to find out what. |
| **Curator** | Publishes a mod list for other people to follow. |
| **Power user** | Comfortable with files, logs and the command line; scripts routine tasks. |

---

## Level 1: Getting started

### US-01 Find my game

As a **new player**, I want the manager to find my Stardew Valley
installation, so that I do not have to know where Steam put it.

- Steam libraries are searched on Windows and Linux; a manual folder can be
  chosen when discovery finds nothing or finds the wrong one.
- The inspection says whether the folder is a fresh native install, already
  modded, a Proton/Wine (Windows) file set on Linux, unreadable or unwritable,
  and why.
- An already modded, unmanaged installation is reported as such and is not
  changed.

### US-02 Install SMAPI for me

As a **new player**, I want the manager to install SMAPI, so that mods can
load without me running an installer by hand.

- A setup preview shows what will change in the game folder before anything
  runs.
- SMAPI's own installer is used, and its download is checked against a
  checksum.
- My mods and profiles are not part of the SMAPI install; the dialog says so.
- If setup is interrupted, the journal is kept. On the next start the manager
  finishes or closes the setup from what it can verify in the game folder,
  and asks for recovery only when it cannot tell, rather than starting again
  blindly.

### US-03 Install my first mod from a ZIP

As a **new player**, I want to pick a mod ZIP I downloaded and install it, so
that I can try my first mod.

- I can choose a ZIP with the picker or drop it on the Mods page; a drop that
  is not exactly one `.zip` says why it was not used.
- Before installing I see what the archive contains: each mod's name, author,
  version and UniqueID, and any requirements it declares.
- An archive with entries that would escape its folder, or that would be
  invalid on Windows, is refused before extraction.
- The install result says what was installed and what, if anything, is still
  missing.

### US-04 Play with mods

As a **new player**, I want one Play button that starts the game with my
mods, so that I do not have to remember how SMAPI is started.

- Launch starts SMAPI directly with the active profile's Mods folder
  (`--mods-path`), never the game's own Mods folder.
- If something blocks the launch (SMAPI missing, files out of sync with the
  recorded state, an unfinished change), the button explains what and how to
  fix it before I click.
- The manager tells me that a modded launch bypasses Steam's overlay and
  playtime tracking.
- Closing the manager never closes the game.

### US-05 Know whether it worked

As a **new player**, I want to see after playing whether my mods actually
loaded, so that I know a silent failure from a success.

- The Overview shows the last session and whether SMAPI's log confirms the
  profile's mods loaded.
- When verification is not possible (anonymised paths, ambiguous identity),
  it says "unavailable" rather than "fine".
- Mods SMAPI skipped are named with SMAPI's reason.

### US-06 Remove a mod I do not want

As a **new player**, I want to remove a mod, so that it no longer loads.

- The confirmation lists every mod in the same package, since they are
  removed together.
- Mods that require the one being removed are named before I confirm.
- The removed folder goes to a recovery folder, not straight to deletion, and
  the removal can be undone from its result.

---

## Level 2: Everyday mod management

### US-07 Turn mods off without uninstalling

As a **regular modder**, I want to disable a mod and enable it later, so
that I can try the game without it and keep its settings.

- Disabling moves the mod's folder out of the scanned Mods path; enabling
  moves it back. Settings stay with it.
- Before it happens I see which mods move with it (shared package), which
  enabled mods would stop loading, and any unmet requirements of what I am
  enabling.
- The change is refused while the game is running.
- Asking again after an interruption completes a half-applied change.

### US-08 Enable or disable many mods at once

As a **regular modder**, I want to select several mods and switch them all
on or off, so that I do not click through them one by one.

- The whole set is reviewed first, including requirements met by another mod
  in the same selection (which are not reported as unmet).
- One failure does not undo the rest; the result lists what changed and what
  did not, and repeating the request finishes the job.
- Progress for a long job is visible from anywhere in the app.

### US-09 Install several ZIPs in one go

As a **regular modder**, I want to install a batch of downloaded mods
together, so that setting up a new playthrough is quick.

- Each archive is checked before anything is installed.
- Mods that need another mod in the batch are installed after it.
- I can stop after the current mod; mods not started are reported as such.
- Each archive gets its own outcome (installed, cannot install, not replaced).

### US-10 Install a missing requirement I already have

As a **regular modder**, I want the manager to offer a requirement it
already has stored, so that I do not hunt for a file I downloaded before.

- When exactly one stored package satisfies the requirement, the offer names
  the package, version and why.
- When none or several fit, it says so and gives the UniqueID to look for,
  instead of guessing a download.

### US-11 Update a mod to a newer version

As a **regular modder**, I want to install a newer ZIP of a mod I already
have, so that I can update it in place.

- The preview shows the installed and incoming versions and whether the new
  one is newer, older, the same or not comparable.
- The mod's `config.json` files are backed up first and kept across the
  replace.
- An automatic restore point is saved before the change, and the change does
  not start if it cannot be saved.
- If the new version fails to install, the previous one is put back.
- A disabled mod stays disabled.

### US-12 Roll back to an older version

As a **regular modder**, I want to go back to an older version of a mod
after an update broke it, so that I can keep playing.

- Installing an older version needs an explicit "I want the older version".
- Settings and rollback-on-failure behave as for an update (US-11).

### US-13 Find a mod quickly

As a **regular modder** with a long mod list, I want to search, sort and
filter, so that I can find one mod among hundreds.

- Search matches every word, ignoring case and accents.
- Sorting by name, author, version, recently installed, favourites or enabled
  first is stable.
- Filters (enabled state, tag, favourites, needs attention) combine, and
  *Show all mods* clears them.
- `Ctrl+K` opens global search across pages, settings, profiles and mods.

### US-14 Keep notes on my mods

As a **regular modder**, I want to star, tag and annotate mods, so that I
remember why I installed something.

- Favourites, tags and notes follow the mod's UniqueID into every profile.
- They never affect installing, enabling or launching.

### US-15 See what a mod depends on

As a **regular modder**, I want a mod's details to show what it requires and
what requires it, so that I understand what will break if I change it.

- Required, optional and content-pack-for relations are listed both ways.
- Missing, disabled and too-old requirements are flagged.
- Mods with a problem show a *Missing requirement* or *Skipped last session*
  badge, and *Needs attention* filters to them.

### US-16 Open a mod's files

As a **regular modder**, I want to open a mod's folder or its original
archive, so that I can edit its config by hand.

- *Show mod folder* and *Show original archive* open the system file manager
  at the right place, whether the mod is enabled or disabled.

### US-17 Keep SMAPI current

As a **regular modder**, I want to be told when a newer SMAPI supports my
game, so that I stay up to date without checking GitHub.

- The release list is read at most once a day.
- By default it only tells; I can opt into automatic updates or never being
  told again.
- Automatic updates wait for the game to close and only install releases with
  a published checksum.
- I can also pick a specific version, repair (reinstall the same version) or
  roll back, and each release says whether it supports my game.

### US-18 Hear about mod updates SMAPI noticed

As a **regular modder**, I want to see the update notices from SMAPI's log,
so that I know which mods have newer versions.

- Notices show the mod, current and available version, taken from the log.
- The manager does not download updates itself.

---

## Level 3: Profiles and playthroughs

### US-19 Keep separate mod sets per playthrough

As a **regular modder**, I want several profiles, each with its own mods, so
that my vanilla-plus farm and my expansion-heavy farm do not interfere.

- Each profile has its own isolated Mods folder in the app data directory.
- The header shows the active profile and switching is one action.
- Profiles can be renamed and described.

### US-20 Link a save to its profile

As a **regular modder**, I want to mark which profile a save is played with,
so that I do not load a farm with the wrong mods.

- Saves are read from the game's save folder without being changed.
- When the most recently played save is linked to a different profile than the
  active one, I am warned, and the warning says it is based on the last played
  save.
- Links survive profile renames; a link to a deleted profile is shown as such.

### US-21 Back up and restore a save

As a **regular modder**, I want to back up a save before a risky change, so
that I can go back if a mod corrupts it.

- Backups are verified file by file before they count as done.
- Restoring first backs up the live save, then replaces it only after the copy
  is checked.
- Both are refused while the game is running.
- Save backups never change installed mods, and profile changes never touch
  saves.

### US-22 Duplicate a profile

As a **regular modder**, I want to copy a profile, so that I can start a new
variant from a set that works.

- The copy installs the same versions from stored packages with the same
  enabled state and settings; the source is untouched and stays active.
- Mods whose package is no longer stored are listed as not copied.
- An interrupted copy appears under *Unfinished copies* and can be finished.

### US-23 Try changes in an experiment

As a **regular modder**, I want to try new mods on a throwaway copy of my
profile, so that my real setup is never at risk.

- *Start an experiment* duplicates and activates a copy; the source is never
  touched.
- *Keep as a normal profile* or *Discard* ends it. Discard reactivates the
  source and deletes the copy through the normal deletion path.
- Nothing from the experiment is copied back automatically.

### US-24 Save and return to restore points

As a **regular modder**, I want named restore points for a profile, so that
I can return to a set I know.

- Up to 20 named and 20 automatic points per profile, newest first. Automatic
  points never push out named ones.
- *Review restore* shows mods to remove, install, change version, enable and
  disable before anything runs.
- A point whose packages are missing or fail their checksum is unavailable as
  a whole; it is never partly restored.
- Restoring saves the current state as a new point first.

### US-25 Go back to the last setup that worked

As a **regular modder**, I want the manager to remember the last profile
state that launched successfully, so that I can compare against it after
something breaks.

- A known-good record is only taken when SMAPI's log confirms the mods loaded.
- The Profiles page shows differences from it.
- *Restore enabled state* switches mods back and turns off mods added since;
  version changes and removed mods are listed for me to reinstall.

### US-26 Freeze a profile

As a **regular modder** mid-playthrough, I want to freeze a profile, so
that I do not accidentally install or remove something.

- While frozen, install and removal previews are refused, including ones
  prepared before the freeze.
- Enabling and disabling remain allowed and are shown as drift from the
  snapshot.
- A *Frozen* badge shows in the header; unfreezing applies nothing.

### US-27 Compare two profiles

As a **regular modder**, I want to compare two profiles, so that I can see
why one works and the other does not.

- Differences are grouped: only in one, different version, same version with a
  different file, enabled in one only, and mod settings.

### US-28 Archive and delete a profile

As a **regular modder**, I want to delete a profile I no longer use, so that
it stops cluttering the list and using disk space.

- Only archived profiles can be deleted, so the active one never is.
- The dialog lists what is removed and what is kept (packages, other profiles,
  SMAPI, the game, saves, history), with the folder size, and needs the name
  typed.
- The folder goes to the trash, kept for a configurable time and recoverable
  from *Recently deleted*.

### US-29 Bring in mods from my old Mods folder

As a **regular modder** coming from a hand-managed setup, I want to adopt
mods from the game's Mods folder into a new profile, so that I do not start
from nothing.

- Scanning only reads. Adopting copies the chosen folders through the normal
  install checks; the original folder is never changed or emptied.
- If a chosen mod needs a mod I am not adopting, I am told before adopting
  that it will not be adopted, because the install checks refuse a mod
  without the mods it needs.
- Mods that cannot be adopted are listed with a reason.
- A mod SMAPI skips because its folder name starts with "." is adopted
  turned off, and the scan says so.

---

## Level 4: Troubleshooting and recovery

### US-30 See the health of my profile

As a **troubleshooter**, I want one health summary, so that I can tell at a
glance if something is wrong.

- Findings cover missing, disabled or too-old requirements, minimum game and
  SMAPI versions, and duplicate UniqueIDs.
- The wording is honest about scope: static checks cannot detect in-game
  conflicts, and an unreadable version is "not assessed", not "fine".
- A finding can be dismissed; dismissals are recorded.

### US-31 Apply the fixes the manager is sure about

As a **troubleshooter**, I want the manager to apply fixes it can make with
certainty, so that I do not have to work them out.

- *Straightforward fixes* lists each fix with the finding behind it, such as
  enabling a required mod that is installed but off.
- It also lists what it leaves alone and why.
- Nothing changes until I confirm.

### US-32 Find the mod causing a problem

As a **troubleshooter**, I want guided bisection of my enabled mods, so that
I can find a broken mod among dozens.

- The original enabled state is saved before the first change, and *Restore
  my original mods* returns to it at any step.
- Dependencies always move with what needs them; mods I had turned off stay
  off.
- If the problem persists with everything off, it reports that the cause is
  probably not a mod.
- A problem needing two mods together ends as "inconclusive" rather than
  blaming one.

### US-33 Test SMAPI on its own

As a **troubleshooter**, I want to start SMAPI with no mods, so that I can
rule SMAPI itself in or out.

- It uses an empty Mods folder; the profile's folder is not touched.
- The result appears with the other sessions.

### US-34 Read the SMAPI log without the noise

As a **troubleshooter**, I want the log summarised by errors, sources and
skipped mods, so that I can see what matters.

- Errors are grouped by source with counts, and skipped mods are listed.
- Two sessions' saved logs can be compared to see what changed between a
  working and a broken launch.
- Errors around a specific operation can be viewed from Activity.

### US-35 Check whether a mod's files were changed

As a **troubleshooter**, I want to compare a mod's files with what was
installed, so that I can spot accidental edits or corruption.

- Each file is reported as missing, changed, settings edited or added since
  install; mods installed before inventories were recorded say *no install
  record*.
- Checking only reads. *Reinstall from archive* restores the files and keeps
  `config.json`.

### US-36 Restore a mod's settings

As a **troubleshooter**, I want to restore a mod's earlier `config.json`, so
that I can undo a settings change that broke it.

- Settings are backed up automatically before every replace and reinstall.
- A backup can be written back into the mod's current folder; nothing else
  changes.

### US-37 Recover from a crash mid-change

As a **troubleshooter**, I want the manager to recover safely after a crash
or power loss during an install or removal, so that my profile is not left
half-changed.

- On start, interrupted operations are found from the journal and the recovery
  screen explains each one.
- Conflicting operations are blocked until recovery finishes.
- I am never told to delete state to get past a recovery error.

### US-38 See everything that happened

As a **troubleshooter**, I want an activity history, so that I can see what
changed and when.

- Each operation lists its steps, effects and outcome.
- New activity since I last looked is marked.

### US-39 Know what the manager does not know

As a **troubleshooter**, I want unknowns listed separately from problems, so
that I do not mistake "not checked" for "fine".

- Each unknown says why it is unknown and what, if anything, would resolve it.

### US-40 Correct a misdetected game version

As a **troubleshooter**, I want to set my game version when detection gets
it wrong, so that compatibility checks use the right one.

- Only the game version can be overridden; paths, ownership and safety checks
  cannot.
- The override is shown wherever it is used and flagged if detection later
  changes.

### US-41 Share a support report

As a **troubleshooter**, I want to send a redacted summary to someone
helping me, so that they can see my setup without my private details.

- The summary and chosen log are shown for review before sharing.
- Personal paths and private details are redacted.
- It can be copied or saved as a bundle; dismissed findings are noted.

---

## Level 5: Sharing and multiplayer

### US-42 Export a recipe of my mod list

As a **multiplayer host**, I want to export a portable list of my mods, so
that friends can see exactly what to install.

- The recipe has names, versions, checksums and enabled state, with no local
  paths, credentials or saves.
- Comparing another recipe with my profile is read-only.

### US-43 Send my whole profile as a bundle

As a **multiplayer host**, I want to send friends a single file with the
mods included, so that they do not have to download each one.

- Export writes a new `.smm-bundle.zip`, never overwriting a file; mods whose
  package is no longer stored are listed as not carried.
- I can mark mods optional; recipients choose them and declined ones are not
  failures.
- Mod settings are left out unless I choose them, and each `config.json` is
  scanned for things that look like tokens, passwords, paths or emails.

### US-44 Import a friend's bundle

As a **multiplayer guest**, I want to import a bundle into a new profile, so
that I match the host without touching my own profiles.

- Import always creates a new profile.
- The bundle is treated as untrusted: only expected entries are accepted, sizes
  are bounded and every package is checked against its digest. A damaged or
  altered bundle is refused before the profile is created.
- I am warned that bundles carry third-party code.
- Each mod that cannot be installed is listed with a reason.
- Mods marked for another operating system are shown as not for my computer.

### US-45 Stay in sync with my group

As a **multiplayer guest**, I want to keep the group's recipe as a
reference, so that I notice when my profile drifts from it.

- Differences by version, checksum and enabled state are shown whenever the
  page is open.
- I can accept a difference for the group; it returns if either side changes.
- A fix is offered once the exact package is stored. Nothing is installed
  automatically.

---

## Level 6: Curating collections

### US-46 Check a profile before sharing it

As a **curator**, I want a pre-share checklist, so that what I publish is
reproducible.

- Checks fail on an empty list, duplicate IDs or blank versions, and warn on
  missing checksums, all mods disabled, no SMAPI version, blank or missing
  names or authors, and nothing marked optional. On a profile, the installer
  already prevents duplicate IDs, blank versions and missing checksums, so
  those checks are a backstop.
- Reproducibility shows the share of mods with both an exact version and a
  checksum; it is never described as a quality or safety score.

### US-47 Publish a collection with notes and options

As a **curator**, I want to publish my profile as a named, revisioned
collection, so that people can follow it.

- Per mod I can add a reason, mark it optional or client-only, allow newer
  versions, add a download link or manual instructions, and share its settings.
- Mods can be put in option groups ("choose any" or "choose one").
- Download links can be checked; a check is a moment in time, not a verdict.
- Each revision has release notes, and a changelog against an earlier
  revision lists added, removed, updated, rolled back and repackaged mods.

### US-48 Explore a collection's dependency graph

As a **curator**, I want to browse the collection's dependency graph, so
that I can see what holds the list together.

- The graph is a filterable, keyboard-focusable list.

### US-49 Follow a collection and take its updates

As a **collection follower**, I want to update to a curator's new revision
while keeping my own tweaks, so that I do not lose my changes.

- The review shows what the curator changed, what I changed and where both
  did.
- Curator changes I had not touched are applied; for each conflict I choose.
- My own changes are kept.
- Everything applied is one journaled change after a restore point.

### US-50 Fork a collection

As a **curator**, I want to base my collection on someone else's, so that I
can publish a variant that credits the original.

- The published recipe records what it was forked from, including its author.
- Forking is a choice: the original's curation is carried over, and a
  profile that follows a collection can publish an unrelated one instead.

---

## Level 7: Power users and maintenance

### US-51 Script the manager from the command line

As a **power user**, I want a CLI for status, profiles, mods and health, so
that I can check my setup from scripts.

- `status`, `profiles`, `mods` and `health` only read; `--json` prints the same
  DTOs the app uses.
- Exit codes distinguish success (0), failure (1), refused (2) and needs
  attention (3).

### US-52 Toggle mods from a script

As a **power user**, I want to enable and disable mods from the command line,
so that I can switch sets before launching.

- Without `--yes` only the plan is printed.
- With `--yes` the same checks apply as in the app (game not running, no
  operation holding the lock, journaled).
- The CLI refuses changes while an interrupted operation waits for the app to
  recover it.

### US-53 See and reclaim disk space

As a **power user**, I want to see what the manager stores and remove what
nothing needs, so that it does not quietly fill my disk.

- Storage usage is broken down by profile (live, disabled, operation data),
  packages and cache.
- Cleanup previews removable items and says why kept items are kept (used by a
  profile, a restore point, a known-good record, or an unresolved operation).
- Cleanup never follows or removes links, and repeating it is harmless.

### US-54 Choose how much recovery data is kept

As a **power user**, I want to set how many backups and how long deleted
profiles are kept, so that I balance safety against disk space.

- Save and settings backups: 1 to 100 kept (default 5). Deleted profiles: 1 to
  3650 days (default 30).
- The policy persists across restarts and only changes what the next cleanup
  offers.

### US-55 Use the manager comfortably

As a **power user** on a handheld or with accessibility needs, I want the
interface to adapt, so that I can use it with a keyboard, at large scale or
on touch.

- System, light and dark themes; UI scale from 90% to 150%.
- Every list and dialog is fully keyboard operable, with focus trapped in
  dialogs and the destructive button never focused first.
- On narrow windows the sidebar collapses to icons; on touch, controls are at
  least 44 px tall.
- *Show guidance text* can be turned off; actions are never hidden.

---

## Out of scope for this version

Stories that users will ask for but that the current release does not
address:

- Downloading mods from Nexus, CurseForge or ModDrop. Updates are checked
  with smapi.io and can be downloaded only from GitHub releases (ADR-0019);
  other updates link to the mod's page.
- Launching through Steam (overlay, playtime) with a per-profile Mods folder.
- macOS, GOG automatic discovery, Flatpak Steam and Proton/Wine installs.
- Sandboxing mods or saying a mod is safe; see `trust-model.md`.
- Detecting in-game conflicts between mods; health checks are static.
