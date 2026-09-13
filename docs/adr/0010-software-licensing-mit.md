# ADR-0010: Software Licensing - MIT License

## Status
Accepted

## Date
2026-09-12

## Context
The Stardew Valley modding community relies heavily on permissive, open-source collaboration. SMAPI itself is licensed under MIT. To facilitate integration with community tooling, packaging by Linux distributions (Fedora, Arch, Debian), and peer review, the project requires a permissive, recognized open-source license.

## Decision
License the Stardew Mod Manager codebase under the **MIT License**.

## Consequences
### Positive
- Maximum compatibility with upstream Stardew modding tools and SMAPI.
- Permissive distribution across Linux software repositories and package managers.
- Clear, simple terms for downstream contributors and packagers.

### Negative
- Does not enforce copyleft protections for proprietary forks (acceptable tradeoff for community desktop utilities).

## Alternatives Considered
- **GPL-3.0**: Stronger copyleft, but creates friction when linking with certain upstream libraries or embedding within broader permissive ecosystems.
- **Apache-2.0**: Explicit patent grants, but slightly more complex than the ubiquitous MIT standard in the Stardew modding community.
