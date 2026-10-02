# Command-line interface

The installed app doubles as a small CLI for scripts:

```
stardew-mod-manager cli <command> [--json] [--yes] [--profile ID]
```

| Command | Changes anything? | What it does |
| --- | --- | --- |
| `status` | no | Manager version, active game and profile, and any interrupted operation |
| `profiles` | no | Profiles for the active game (`*` marks the active one) |
| `mods` | no | Mods in the active profile, or the one given with `--profile` |
| `health` | no | Health findings for the profile |
| `enable <UniqueID>...` | with `--yes` | Turns mods on |
| `disable <UniqueID>...` | with `--yes` | Turns mods off |

`--json` prints the same data the app uses (the generated DTOs) for every
command, including the plan for `enable` and `disable`.

## Safety

- `enable` and `disable` only print the plan unless `--yes` is given. With
  `--yes` they go through the same toggle service as the app, so the same
  checks apply: the game must not be running, no other operation may hold the
  lock, and the change is journaled.
- The CLI never recovers interrupted operations, because the app may be open
  and still carrying one out. It reports them (`status` exits with 3) and
  refuses changes until the app has recovered them.
- The CLI never accepts credentials.

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | Success |
| 1 | The manager could not do it (files, storage, internal error) |
| 2 | Invalid or refused: unknown command or mod, no active profile, game running, operation in progress |
| 3 | Needs attention: an interrupted operation waits for recovery, or `health` found warnings or errors |

Errors go to standard error, as JSON (`code`, `category`, `summary`) with
`--json`.
