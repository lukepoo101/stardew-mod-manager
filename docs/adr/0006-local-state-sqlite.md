# ADR-0006: Local State Persistence - SQLite with Embedded Migrations

## Status
Accepted

## Date
2026-09-12

## Context
The manager must maintain persistent metadata about detected game installations, SMAPI setups, installed mods, file manifests, operation journals, and launch session logs. State operations must support relational integrity (e.g. foreign keys linking installed mod files to mod records), crash resilience, and ACID transactions for multi-row mutations.

## Decision
Use **SQLite** via `rusqlite` (with `bundled` feature flag) stored in the standard user data directory (`~/.local/share/stardew-mod-manager/manager.db` on Linux). Apply embedded schema migrations on startup with `PRAGMA foreign_keys = ON`.

## Consequences
### Positive
- Zero external database daemon dependencies; fully embedded single-file storage.
- Relational integrity with cascading foreign key guarantees.
- Atomic commit of mod installs and uninstall transactions (`db.commit_install_transaction`, `db.commit_remove_transaction`).
- Journaling of pending filesystem mutations provides crash recovery points on startup.

### Negative
- Concurrent writes from multiple processes are serialized by SQLite locks (mitigated by our application-level `fs2` advisory lock).
- Database migrations must be maintained cleanly over application releases.

## Alternatives Considered
- **Plain JSON / TOML files**: Susceptible to corruption during power loss or abrupt termination; lack transactional multi-file integrity and relational queries.
- **Key-Value Stores (sled / rocksdb)**: Lacks simple relational queries and ad-hoc schema inspection.
