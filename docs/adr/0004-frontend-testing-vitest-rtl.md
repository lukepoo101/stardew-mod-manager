# ADR-0004: Frontend Test Framework - Vitest and React Testing Library

## Status
Accepted

## Date
2026-09-12

## Context
Automated verification of the user interface requires unit and integration tests for component rendering, user interactions, wizard step transitions, accessibility attributes, and mock backend communications. The testing harness must integrate seamlessly with the Vite build pipeline without dual compilation or separate babel configurations.

## Decision
Adopt **Vitest** paired with **@testing-library/react** and **jsdom** for frontend testing.

## Consequences
### Positive
- Reuses the existing `vite.config.ts` configuration, plugins, and path aliases.
- Extremely fast execution with ESM native execution and multi-threaded worker pools.
- Standard Jest-compatible API (`describe`, `it`, `expect`, `vi`).
- User-centric testing paradigm via React Testing Library encourages testing behavior rather than implementation details.

### Negative
- jsdom does not fully simulate all browser layout engine quirks (e.g. detailed CSS subpixel measurements).

## Alternatives Considered
- **Jest**: Requires separate TypeScript preprocessors (`ts-jest` or `@swc/jest`) and struggles with Vite ESM configurations.
- **Playwright / Cypress Component Testing**: Highly capable for full browser E2E, but heavier and slower for fast local unit and regression testing.
