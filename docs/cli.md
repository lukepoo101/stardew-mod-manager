# Command-line interface

The installed app doubles as a small CLI for scripts:

```
stardew-mod-manager cli <command> [--json] [--yes] [--profile P]
```

| Command | Changes anything? | What it does |
| --- | --- | --- |
| `status` | no | Manager version, active game folder and profile, whether the app is open, and every unfinished change or unfinished profile copy |
| `profiles` | no | Profiles for the active game (`*` marks the active one; archived ones are marked) |
| `mods` | no | Mods in the active profile, or the one given with `--profile` |
| `health` | no | Health findings for the active profile, or the one given with `--profile` |
| `enable <UniqueID>...` | with `--yes` | Turns mods on |
| `disable <UniqueID>...` | with `--yes` | Turns mods off |
| `help` | no | Usage |

`--profile` takes a profile's id or its name (as `profiles` lists them). An
unknown profile is refused; it is never read as an empty one.

`--json` prints the same data the app uses (the generated DTOs) for every
command. `status` prints `CliStatusDto` (the app's bootstrap data plus
`app_open`, `game_path`, `active_profile_name`, `unfinished_operations` and
`unfinished_copies`); `enable` and `disable` print `CliTogglePlanDto`.

## Unfinished changes

`status` lists each unfinished change with one of these statuses:

| Status | Meaning | Exit code |
| --- | --- | --- |
| `needs_recovery` | the app could not prove how it ended; open the app to recover it | 3 |
| `interrupted` | the app stopped (or crashed) while doing it; it is recovered when the app next starts | 3 |
| `in_progress` | the open app (or another command) is doing it right now | 0 |
| `open_review` | an install or removal review nobody confirmed; nothing has changed | 0 |

Whether the app is open is known from the lock an open window holds for its
whole life. An open review never blocks a change: a change from here cancels
it, because it could no longer be confirmed anyway.

## Safety

- `enable` and `disable` only print the plan unless `--yes` is given. The plan
  shows what the app shows before the same change: mods that move with it
  because they came in the same package, enabled mods that will stop
  loading, and requirements left unmet. With `--yes` they go through the same
  toggle service as the app, so the same checks apply: the game must not be
  running, no other change may be in progress on the profile, and the change
  is journaled.
- The CLI never recovers interrupted operations, because the app may be open
  and still carrying one out. It reports them (`status` exits with 3) and
  refuses changes until the app has recovered them.
- Reading never sets the manager up: on a computer where the app has never
  run, `status` says so and nothing is created.
- An open app window notices changes made here within about two seconds and
  refreshes.
- The CLI never accepts credentials.

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | Success |
| 1 | The manager could not do it (files, storage, internal error), or some of the mods named could not be changed |
| 2 | Invalid or refused: unknown command, option, mod or profile, no active profile, game running, another change in progress |
| 3 | Needs attention: an interrupted operation waits for recovery, a profile copy is unfinished, `health` found warnings or errors, or a change with `--yes` left an enabled mod without something it needs |

Errors go to standard error, as JSON (`code`, `category`, `summary`) with
`--json`, including mistakes in the command line itself.
