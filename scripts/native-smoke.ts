// Entry point for the packaged-application smoke test.
//
//   bun scripts/native-smoke.ts --output <dir> [--binary <path>] [--direct-start]
//       [--webview2-isolation <dir>] [--driver-provider auto|embedded|external]
//       [--require-webview2]
//
// The implementation lives in scripts/native-smoke/: this file exists so the
// documented invocation works from the repository root, and so the platform
// dispatch happens in one place (native-smoke/run.ts).

import { reportUncaught } from "./native-smoke/py.ts";
import { main } from "./native-smoke/run.ts";

let status: number;
try {
  status = await main(process.argv.slice(2));
} catch (error) {
  status = reportUncaught(error);
}
process.exit(status);
