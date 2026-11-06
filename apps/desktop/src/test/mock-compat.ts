// Bridges a gap between Bun's mock functions and Vitest's that the suite
// depends on. Imported once by setup.ts.
//
// In Vitest, mockReturnValue, mockResolvedValue and mockRejectedValue (and
// their Once forms) are shorthands for mockImplementation(Once), so
// getMockImplementation() returns the function they installed. Bun 1.4.4
// stores the value elsewhere and getMockImplementation() returns undefined,
// which breaks tests that read back a mocked report to build a variant of it
// (Diagnostics.test.tsx). Routing the shorthands through mockImplementation
// restores Vitest's behaviour; calls, results and restore are unaffected.
//
// Bun's shared mock prototype is non-writable and non-configurable, so the
// shorthands are defined on each mock that vi.fn and vi.spyOn return.
import { vi } from "bun:test";

type AnyFn = (...args: unknown[]) => unknown;
type MockLike = {
  mockImplementation(fn: AnyFn): unknown;
  mockImplementationOnce(fn: AnyFn): unknown;
};

const shorthands: Record<string, (value: unknown) => AnyFn> = {
  mockReturnValue: (value) => () => value,
  mockResolvedValue: (value) => () => Promise.resolve(value),
  mockRejectedValue: (value) => () => Promise.reject(value),
};

function withVitestShorthands<T>(mock: T): T {
  const target = mock as T & MockLike;
  for (const [name, toImplementation] of Object.entries(shorthands)) {
    if (!(name in target) || !(`${name}Once` in target)) {
      throw new Error(`bun:test mocks have no ${name}; revisit mock-compat.ts`);
    }
    Object.defineProperty(target, name, {
      value: (value: unknown) =>
        target.mockImplementation(toImplementation(value)),
      configurable: true,
      writable: true,
    });
    Object.defineProperty(target, `${name}Once`, {
      value: (value: unknown) =>
        target.mockImplementationOnce(toImplementation(value)),
      configurable: true,
      writable: true,
    });
  }
  return mock;
}

const { fn, spyOn } = vi;
vi.fn = ((...args: Parameters<typeof fn>) =>
  withVitestShorthands(fn(...args))) as typeof fn;
vi.spyOn = (<T extends object, K extends keyof T>(obj: T, key: K) =>
  withVitestShorthands(spyOn(obj, key))) as typeof spyOn;
