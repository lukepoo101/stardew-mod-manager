# ADR-0004: Frontend Test Framework - `bun test` and React Testing Library

## Status
Accepted — amended 2026-11-03: the runner moved from Vitest to `bun test`. Testing Library, jsdom and the tests' assertions are unchanged.

## Date
2026-09-12 (amended 2026-11-03)

## Context
Automated verification of the user interface requires unit and integration tests for component rendering, user interactions, wizard step transitions, accessibility attributes, and mock backend communications.

The original decision chose Vitest because it reused the Vite pipeline. Once Bun became the package manager and script runner, a second runtime (Node) and a second module pipeline (Vite's transform) existed only to run the tests. Bun's built-in runner transpiles TypeScript and JSX itself and implements the Jest/Vitest API the suite uses (`describe`, `it`, `expect`, `vi.fn`, `vi.spyOn`, `vi.mock`).

## Decision
Run the frontend tests with **`bun test`**, paired with **@testing-library/react**, **@testing-library/jest-dom** and **jsdom**, the same DOM as before. `apps/desktop` runs them with `bun run test`, which is `bun test --parallel`.

How it is wired (`apps/desktop/bunfig.toml`, `src/test/`):

- **One jsdom window per test file.** Bun 1.4.4 has no DOM environment of its own. Its newer docs list `[test] environment`, but 1.4.4 silently ignores that key. `src/test/dom-environment.ts` is preloaded instead. It ports Vitest's jsdom environment: the same `JSDOM` options, and window properties installed on `globalThis` by Vitest's rule (DOM classes replace the runtime's, everything else only fills gaps). The one deliberate difference is that functions and classes are plain data properties rather than getters, because Bun's `spyOn` cannot spy on accessors (`window.confirm` and friends).
- **Per-file isolation is required.** `--parallel` implies `--isolate`, so each file gets a fresh global and module registry, as each Vitest worker did. Plain `bun test` shares one global and fails, and Bun has no bunfig key for either flag. `bun test --isolate` (serial) also passes, but took 60 s against 7 s here.
- **`src/test/setup.ts`** registers the jest-dom matchers on Bun's `expect`. It does by hand what Testing Library's auto-setup does under Vitest's `globals: true`: cleanup after each test and React's act() environment. Bun does not expose the hooks as globals. It also sets a 5 s Testing Library wait (`asyncUtilTimeout`) and a 30 s per-test timeout, because the suite runs on busy machines.
- **`src/test/bun-test.d.ts`** types `bun:test` and the jest-dom matchers for `tsc`.
- Test files import from `"bun:test"`, so `tsc` checks them against the API that actually runs.

### Bridging Vitest's `vi` API

| Vitest feature used | Bun 1.4.4 | Bridge |
|---|---|---|
| `vi.fn`, `vi.spyOn`, `vi.restoreAllMocks`, `vi.clearAllMocks`, `mockImplementation(Once)`, `mockResolvedValueOnce`, `mock.calls`, `mock.results`, the `*Called*` matchers | present | none |
| `getMockImplementation()` after `mockReturnValue`/`mockResolvedValue`/`mockRejectedValue` (3 uses, `Diagnostics.test.tsx`) | returns `undefined`, because Bun stores the value outside the implementation | `src/test/mock-compat.ts` wraps `vi.fn` and `vi.spyOn` so those shorthands (and their Once forms) route through `mockImplementation(Once)`, as Vitest defines them. Bun's mock prototype methods are non-writable and non-configurable, so they are defined on each mock |
| `vi.mocked(fn)` (2 uses, `SmapiStatus.test.tsx`) | missing | replaced at the call site by `fn as Mock<typeof fn>`. In Vitest it is an identity function that exists only for typing. Bun's `vi` is typed as a closed object, so a shim could not be typed |
| `vi.hoisted(factory)` (2 uses, `BackendEvents.test.tsx`, `OnboardingSetup.test.tsx`) | missing | replaced by an immediately-invoked function. Bun does not hoist `vi.mock`. It registers the mock when the call runs and rebinds modules already imported, so the factory's state just needs to exist first. Seeding a broken factory fails those tests, so the mocks do take effect |
| `vi.mock(path, factory)` | present (`mock.module`) | none |
| `mock.results[i].value` typed `any` | typed `unknown` | type casts at two call sites (`App.test.tsx`, `Reinstall.test.tsx`), plus a parameter list on one `vi.fn` (`ApiContracts.test.ts`) so `mock.calls` matches what `toEqual` compares |
| Fake timers | `vi.useFakeTimers` and friends exist | unused by this suite, so unverified |

### Coverage
`bun run test --coverage` writes a text summary and `coverage/lcov.info`. It covers the same 129 source files that `@vitest/coverage-v8` reported, with test files and `src/test/` support left out. Compared with coverage-v8, it loses:
- **branch and statement counts.** Bun reports functions and lines only.
- **the HTML reporter** (use an LCOV viewer instead).
- **comparable numbers.** Bun counts lines and functions from JavaScriptCore, so its percentages are not on V8's scale. On the same suite, Bun reported 85.2 % functions and 89.1 % lines, and V8 reported 75.9 % functions and 80.8 % lines.

No CI job collects coverage, before or after this change.

## Consequences
### Positive
- One runtime for install, scripts and tests, and no Vite transform in the test path.
- Faster runs than Vitest on Node, measured during the Bun migration.
- The same assertions as before: all 428 tests pass under jsdom.

### Negative
- The jsdom environment is our code (`dom-environment.ts`), not the runner's. A jsdom or Bun upgrade can break it in ways Vitest would have absorbed.
- `mock-compat.ts` depends on Bun's mock internals: the methods it shadows must exist on each mock, and it throws if they disappear. Re-check it on every Bun upgrade; delete it once Bun's `getMockImplementation()` reflects the shorthands.
- Bun prints console output from passing tests, including React's act() warnings that Vitest 5's default reporter hid. The warnings were already there; only the log is noisier.
- jest-dom failure messages are printed by Bun's matcher utilities and read slightly differently.
- Plain `bun test` in `apps/desktop` fails. Use `bun run test`.
- `@types/bun` adds the `Bun` global to the type-check of the application code, as `@types/node` already did for `process` and `Buffer`.
- jsdom does not fully simulate all browser layout engine quirks (e.g. detailed CSS subpixel measurements).

## Alternatives Considered
- **Vitest on Bun's runtime (`bun --bun vitest run`)**: zero test changes and about 30 % faster than Vitest on Node, but it keeps Vitest and its Vite transform, and it runs Vitest outside its tested runtime matrix.
- **Vitest on Node (the original decision)**: the reference behaviour, but it needs Node and its own module pipeline alongside Bun.
- **happy-dom instead of jsdom**: about 2.4× faster than jsdom under `bun test`, but it changes accessibility semantics. The accessible name of a checkbox in `Adoption.test.tsx` is not found.
- **Jest**: requires separate TypeScript preprocessors (`ts-jest` or `@swc/jest`) and struggles with Vite ESM configurations.
- **Playwright / Cypress Component Testing**: highly capable for full browser E2E, but heavier and slower for fast local unit and regression testing.
