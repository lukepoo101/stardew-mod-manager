import "@testing-library/jest-dom/vitest";
import { configure } from "@testing-library/react";

// The default one-second wait for findBy and waitFor is shorter than a busy
// machine needs to render a multi-step dialog, which made a few tests fail
// only under load. Five seconds still fails a test that never renders.
configure({ asyncUtilTimeout: 5000 });
