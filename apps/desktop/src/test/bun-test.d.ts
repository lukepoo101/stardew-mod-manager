// Types for the test files: Bun's `bun:test` module, and the jest-dom
// matchers that setup.ts registers on its `expect`. This mirrors
// @testing-library/jest-dom/types/bun.d.ts, which the package does not
// export.
/// <reference types="bun" />
import type { expect } from "bun:test";
import type { TestingLibraryMatchers } from "@testing-library/jest-dom/matchers";

declare module "bun:test" {
  interface Matchers<T = unknown>
    extends TestingLibraryMatchers<
      ReturnType<typeof expect.stringContaining>,
      T
    > {}
}
