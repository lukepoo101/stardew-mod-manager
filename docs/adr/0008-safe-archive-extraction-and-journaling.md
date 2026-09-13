# ADR-0008: Archive Extraction and File Operations - Safe Filesystem Copy and Journaling

## Status
Accepted

## Date
2026-09-12

## Context
User-provided ZIP archives can be malformed or deliberately adversarial (Zip Slip, zip bombs, symlink attacks, reserved characters, NUL bytes). Furthermore, extracting and copying mod trees onto the filesystem is not atomic: a sudden power loss, process termination, or crash halfway through extraction leaves partial files on disk.

## Decision
1. **Adversarial ZIP Protection**:
   - Enforce resource limits: maximum 512 MB compressed size, 2 GB uncompressed size, 20,000 maximum file entries.
   - Strict path sanitation: reject any relative path components (`..`), absolute paths, Windows UNC/drive specifiers (`C:\`), NUL bytes, and symlinks/hardlinks.
2. **Staged Extraction & Inspection**:
   - Extract ZIP archives exclusively to a private temporary directory (`<cache_dir>/staging/<uuid>/`).
   - Validate `manifest.json`, minimum SMAPI requirements, dependencies, and EntryDll existence before any permanent move.
3. **Journaled Operations & Crash Recovery**:
   - Record pending file operations (`Install`, `Remove`) in SQLite before touching the managed mod directory.
   - If the process crashes mid-operation, the startup recovery pass (`recover_operations`) detects interrupted journals and cleans up partial directory trees.

## Consequences
### Positive
- Prevents directory traversal and denial-of-service via zip bombs.
- Broken or incompatible archives are rejected before the filesystem or database is modified.
- No dangling partial mod folders after unexpected crashes.

### Negative
- Temporary staging requires disk space equal to the uncompressed mod size during inspection.
- Staging and moving requires a file copy step across distinct filesystems if temp and data dirs are on separate partitions.

## Alternatives Considered
- **Direct in-place extraction**: Highly dangerous; partial extractions pollute the mod directory and cannot be cleanly rolled back on failure.
