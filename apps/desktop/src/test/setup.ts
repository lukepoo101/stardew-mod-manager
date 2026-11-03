import "@testing-library/jest-dom/vitest";
import { configure } from "@testing-library/react";

// The default one-second wait for findBy and waitFor is shorter than a busy
// machine needs to render a multi-step dialog, which made tests fail only
// under load; five seconds was still too short with dozens of compilers
// running. A test that passes is not slowed down by a longer wait, and one
// that never renders still fails well inside the 30-second test timeout.
configure({ asyncUtilTimeout: 15_000 });
