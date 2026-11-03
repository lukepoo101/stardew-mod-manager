// Shared set-up for every test file, preloaded by bunfig.toml after
// dom-environment.ts. It does what Vitest's `globals: true`, `testTimeout`
// and the jest-dom Vitest entry point did before (see ADR-0004).
import {
  afterAll,
  afterEach,
  beforeAll,
  expect,
  setDefaultTimeout,
} from "bun:test";
import * as matchers from "@testing-library/jest-dom/matchers";
import { cleanup, configure } from "@testing-library/react";
import "./mock-compat";

expect.extend(matchers);

// Testing Library unmounts after each test and turns on React's act()
// environment only when it finds the test hooks as globals. Bun does not
// install them as globals, so register both here, as its auto-setup would.
const globals = globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean };
let previousActEnvironment: boolean | undefined;
beforeAll(() => {
  previousActEnvironment = globals.IS_REACT_ACT_ENVIRONMENT;
  globals.IS_REACT_ACT_ENVIRONMENT = true;
});
afterEach(() => {
  cleanup();
});
afterAll(() => {
  globals.IS_REACT_ACT_ENVIRONMENT = previousActEnvironment;
});

// The default one-second wait for findBy and waitFor is shorter than a busy
// machine needs to render a multi-step dialog, which made a few tests fail
// only under load. Five seconds still fails a test that never renders.
configure({ asyncUtilTimeout: 5000 });

// Well above the five-second query wait, so a slow machine fails a test on
// what it asserts rather than on the runner's clock.
setDefaultTimeout(30_000);
