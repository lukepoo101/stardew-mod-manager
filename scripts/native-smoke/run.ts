// Run the packaged-application smoke test for the current platform.
//
// Usage:
//     bun scripts/native-smoke.ts --binary <path> --output <dir> \
//         [--direct-start] [--require-webview2]
//
// Two claims can be checked, and they are not the same claim:
//
// * `--direct-start` launches the installed executable the way a user does and
//   requires it to still be running after 15 seconds. It needs no driver, so it
//   is the gate that always has to hold.
// * `--require-webview2` additionally drives the application through Edge
//   Driver and completes the user journey. It needs a host that lets the
//   WebView2 runtime initialise inside a WebDriver-launched process, which not
//   every host permits, and it reports `blocked` rather than `failed` when that
//   is what happened.
//
// The platform module owns the driver and the fixture layout; the user journey
// is shared, so a failure here means the packaged application is broken rather
// than that two scripts disagreed.
//
// Outcomes and exit codes (unchanged from the Python original):
// * exit 0 with result.json `status: passed` (or `blocked` on Windows);
// * exit 1 on any failure, with the error as the last line on stderr and no
//   new result.json;
// * exit 2 on a command-line error, with argparse's usage and message.

import { mkdirSync, rmSync } from "node:fs";
import { join } from "node:path";
import * as linux from "./linux.ts";
import { IS_WINDOWS, print, pyJsonDumps, pyRepr, writeText } from "./py.ts";
import * as windows from "./windows.ts";

const PROG = "native-smoke";
const PROVIDER_CHOICES = ["auto", "embedded", "external"];

const USAGE = `usage: ${PROG} [-h] [--binary BINARY] --output OUTPUT [--direct-start]
                    [--webview2-isolation WEBVIEW2_ISOLATION]
                    [--driver-provider {auto,embedded,external}]
                    [--require-webview2]
`;

const HELP = `${USAGE}
options:
  -h, --help            show this help message and exit
  --binary BINARY
  --output OUTPUT
  --direct-start        Launch the installed executable directly and require
                        it to stay alive; makes no use of a WebDriver.
  --webview2-isolation WEBVIEW2_ISOLATION
                        Directory that holds one isolated WebView2 user-data
                        directory per launch. Defaults to
                        <output>/webview2-isolation.
  --driver-provider {auto,embedded,external}
                        How to drive the application. "embedded" uses the
                        WebDriver server the application hosts itself behind
                        the webdriver cargo feature, "external" uses tauri-
                        driver and the platform driver, and "auto" prefers
                        embedded and falls back. Only meaningful on Windows.
  --require-webview2    Fail instead of skipping when no WebDriver provider is
                        available, and report an environment that cannot host
                        a WebView2 window as blocked rather than as a failure.
`;

export interface Arguments {
  binary: string | null;
  output: string;
  directStart: boolean;
  webview2Isolation: string | null;
  driverProvider: string;
  requireWebview2: boolean;
}

/** An argparse exit: the status, and what to write where. */
export class ArgumentExit extends Error {
  constructor(
    readonly status: number,
    readonly stdout = "",
    readonly stderr = "",
  ) {
    super(`argument parsing exited with ${status}`);
  }
}

type Option = {
  flag: string;
  dest: keyof Arguments | "help";
  takesValue: boolean;
};

const OPTIONS: Option[] = [
  { flag: "--help", dest: "help", takesValue: false },
  { flag: "--binary", dest: "binary", takesValue: true },
  { flag: "--output", dest: "output", takesValue: true },
  { flag: "--direct-start", dest: "directStart", takesValue: false },
  { flag: "--webview2-isolation", dest: "webview2Isolation", takesValue: true },
  { flag: "--driver-provider", dest: "driverProvider", takesValue: true },
  { flag: "--require-webview2", dest: "requireWebview2", takesValue: false },
];

function fail(message: string): never {
  throw new ArgumentExit(2, "", `${USAGE}${PROG}: error: ${message}\n`);
}

/** Whether argparse would read this word as an option rather than a value. */
function looksLikeOption(word: string): boolean {
  if (!word.startsWith("-") || word === "-") return false;
  if (/^-\d+$|^-\d*\.\d+$/.test(word)) return false;
  return !word.includes(" ");
}

/** argparse's lookup: exact flag, `--flag=value`, or a unique prefix. */
function matchOption(
  word: string,
): { option: Option; inline: string | null } | null {
  if (word === "-h") return { option: OPTIONS[0] as Option, inline: null };
  if (!word.startsWith("--")) return null;
  const equals = word.indexOf("=");
  const name = equals === -1 ? word : word.slice(0, equals);
  const inline = equals === -1 ? null : word.slice(equals + 1);
  const exact = OPTIONS.find((option) => option.flag === name);
  if (exact) return { option: exact, inline };
  const prefixed = OPTIONS.filter((option) => option.flag.startsWith(name));
  if (prefixed.length > 1) {
    const matches = prefixed.map((option) => option.flag).join(", ");
    fail(`ambiguous option: ${name} could match ${matches}`);
  }
  const only = prefixed[0];
  return only ? { option: only, inline } : null;
}

/** The same command line, the same errors and the same exit codes as argparse. */
export function parseArguments(argv: string[]): Arguments {
  // argparse classifies every word first, so an ambiguous prefix is reported
  // before anything else.
  const words = argv.map((word, index) => {
    const afterSeparator = argv.slice(0, index).includes("--");
    return afterSeparator || word === "--" || !looksLikeOption(word)
      ? null
      : matchOption(word);
  });

  const parsed: Partial<Arguments> = {};
  const extras: string[] = [];
  for (let i = 0; i < argv.length; i++) {
    const word = argv[i] as string;
    const match = words[i];
    if (match === null || match === undefined) {
      extras.push(word);
      continue;
    }
    const { option, inline } = match;
    if (option.dest === "help") {
      throw new ArgumentExit(0, HELP);
    }
    if (!option.takesValue) {
      if (inline !== null)
        fail(
          `argument ${option.flag}: ignored explicit argument ${pyRepr(inline)}`,
        );
      (parsed as Record<string, unknown>)[option.dest] = true;
      continue;
    }
    let value = inline;
    if (value === null) {
      const next = argv[i + 1];
      if (
        next === undefined ||
        (looksLikeOption(next) && !argv.slice(0, i + 1).includes("--"))
      ) {
        fail(`argument ${option.flag}: expected one argument`);
      }
      value = next;
      i += 1;
    }
    if (option.dest === "driverProvider" && !PROVIDER_CHOICES.includes(value)) {
      fail(
        `argument ${option.flag}: invalid choice: ${pyRepr(value)} (choose from ${PROVIDER_CHOICES.join(", ")})`,
      );
    }
    (parsed as Record<string, unknown>)[option.dest] = value;
  }
  if (parsed.output === undefined) {
    fail("the following arguments are required: --output");
  }
  if (extras.length > 0) {
    fail(`unrecognized arguments: ${extras.join(" ")}`);
  }
  // pathlib reads an empty path as the current directory.
  const path = (value: string) => (value === "" ? "." : value);
  return {
    binary: parsed.binary == null ? null : path(parsed.binary),
    output: path(parsed.output),
    directStart: parsed.directStart ?? false,
    webview2Isolation:
      parsed.webview2Isolation == null ? null : path(parsed.webview2Isolation),
    driverProvider: parsed.driverProvider ?? "auto",
    requireWebview2: parsed.requireWebview2 ?? false,
  };
}

export async function main(
  argv: string[] = process.argv.slice(2),
): Promise<number> {
  let args: Arguments;
  try {
    args = parseArguments(argv);
  } catch (error) {
    if (!(error instanceof ArgumentExit)) throw error;
    process.stdout.write(error.stdout);
    process.stderr.write(error.stderr);
    return error.status;
  }

  let result: Record<string, unknown>;
  if (IS_WINDOWS) {
    const binary = args.binary ?? windows.DEFAULT_BINARY;
    result = await windows.run(binary, args.output, {
      requireWebview2: args.requireWebview2,
      directStart: args.directStart,
      isolationRoot: args.webview2Isolation,
      driverProvider: args.driverProvider,
    });
  } else {
    const binary = args.binary ?? linux.DEFAULT_BINARY;
    result = await linux.run(binary, args.output);
  }

  mkdirSync(args.output, { recursive: true });
  const payload = `${pyJsonDumps(result, 2)}\n`;
  writeText(join(args.output, "result.json"), payload);
  for (const name of ["native-failure.png", "native-failure.txt"]) {
    rmSync(join(args.output, name), { force: true });
  }
  print(payload);
  return 0;
}
