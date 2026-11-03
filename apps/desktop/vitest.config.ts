import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import path from "path";

export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test/setup.ts"],
    // Well above the five-second query wait in setup.ts, so a slow machine
    // fails a test on what it asserts rather than on the runner's clock.
    testTimeout: 30_000,
    // One jsdom worker per core starved each other on a busy machine and
    // timed out tests that pass alone; half the cores keeps a full run fast
    // without that. CI runners have few cores, so nothing changes there.
    maxWorkers: "50%",
  },
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
});
