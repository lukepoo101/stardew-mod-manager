# ADR-0003: JavaScript Tooling - Node.js 24 LTS and pnpm

## Status
Accepted

## Date
2026-09-12

## Context
Earlier draft proposals evaluated alternative JavaScript runtimes such as Bun. However, enterprise desktop build infrastructure, CI/CD runners on Fedora/RHEL, and Tauri CLI tooling standardly depend on Node.js. Furthermore, modern package management requires deterministic locking, fast content-addressable storage, strict dependency resolution, and workspace monorepo support.

## Decision
Standardize on **Node.js 24 LTS** as the runtime environment and **pnpm** (v11+) as the package manager across all desktop frontend development and packaging workflows.

## Consequences
### Positive
- Strict isolation: `pnpm` avoids phantom dependencies via symlinked `node_modules`.
- Storage efficiency: Hard links to a central content-addressable store minimize disk usage.
- Deterministic builds: Consistent `pnpm-lock.yaml` across developer workstations and CI pipelines.
- Standard integration with Tauri 2 CLI (`pnpm tauri build`).

### Negative
- `pnpm` requires explicit configuration for build scripts (`allowBuilds` in `pnpm-workspace.yaml`).
- Developers must have `pnpm` installed or utilize corepack.

## Alternatives Considered
- **Bun**: Fast runtime, but introduces compatibility variations with certain native Node tooling and less mature long-term CI enterprise support.
- **npm / yarn**: Standard, but slower and less strictly isolated than pnpm's content-addressable symlink tree.
