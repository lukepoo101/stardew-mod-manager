// Starting a Windows process with creation flags, which Bun.spawn cannot do.
//
// Python's subprocess.Popen took `creationflags`, and the Windows smoke test
// depended on two of them:
//
// * CREATE_NEW_CONSOLE for the application driven through its embedded
//   WebDriver server. The application starts the SMAPI installer with its own
//   standard handles (crates/manager-infra/src/smapi_adapter), and the
//   installer only runs when those handles are not redirected: it calls
//   Console.Clear(), and on failure Console.ReadKey(), and both need a real
//   console. Bun.spawn always passes STARTF_USESTDHANDLES, so the application,
//   and through it the installer, got Bun's pipes (or NUL) instead, and the
//   installer died with 0xE0434352. Here the application is started the way
//   Popen started it: a new console, no handle redirection, no inheritance.
// * CREATE_NEW_PROCESS_GROUP for the direct start and for tauri-driver, whose
//   output is redirected to log files. As Popen did, only the redirection
//   handles are inherited (PROC_THREAD_ATTRIBUTE_HANDLE_LIST), so the child
//   cannot hold any of Bun's own handles open.
//
// The returned object has the part of Bun's Subprocess interface the smoke
// test uses, so the process helpers treat both alike.

import { resolve } from "node:path";
import type { ChildProcess } from "./process.ts";

export const CREATE_NEW_CONSOLE = 0x0000_0010;
export const CREATE_NEW_PROCESS_GROUP = 0x0000_0200;
const CREATE_UNICODE_ENVIRONMENT = 0x0000_0400;
const EXTENDED_STARTUPINFO_PRESENT = 0x0008_0000;
const STARTF_USESTDHANDLES = 0x0000_0100;
const PROC_THREAD_ATTRIBUTE_HANDLE_LIST = 0x0002_0002n;

const GENERIC_READ = 0x8000_0000;
const GENERIC_WRITE = 0x4000_0000;
const FILE_SHARE_ALL = 0x1 | 0x2 | 0x4;
const CREATE_ALWAYS = 2;
const OPEN_EXISTING = 3;
const FILE_ATTRIBUTE_NORMAL = 0x80;
const INVALID_HANDLE_VALUE = 0xffff_ffff_ffff_ffffn;
const WAIT_OBJECT_0 = 0;

// x64 layouts.
const STARTUPINFOW_SIZE = 104;
const STARTUPINFOEXW_SIZE = 112;
const PROCESS_INFORMATION_SIZE = 24;
const SECURITY_ATTRIBUTES_SIZE = 24;

// How often a running child is checked for having exited.
const POLL_MILLISECONDS = 50;

type Pointer = Uint8Array | BigUint64Array | Uint32Array | null;

type Kernel32 = {
  CreateProcessW: (
    application: Pointer,
    commandLine: Pointer,
    processAttributes: Pointer,
    threadAttributes: Pointer,
    inheritHandles: number,
    flags: number,
    environment: Pointer,
    directory: Pointer,
    startupInfo: Pointer,
    information: Pointer,
  ) => number;
  CreateFileW: (
    path: Pointer,
    access: number,
    share: number,
    attributes: Pointer,
    disposition: number,
    flags: number,
    template: bigint,
  ) => bigint;
  CloseHandle: (handle: bigint) => number;
  WaitForSingleObject: (handle: bigint, milliseconds: number) => number;
  GetExitCodeProcess: (handle: bigint, code: Pointer) => number;
  TerminateProcess: (handle: bigint, code: number) => number;
  GetLastError: () => number;
  InitializeProcThreadAttributeList: (
    list: Pointer,
    count: number,
    flags: number,
    size: Pointer,
  ) => number;
  UpdateProcThreadAttribute: (
    list: Pointer,
    flags: number,
    attribute: bigint,
    value: Pointer,
    size: bigint,
    previous: Pointer,
    returnSize: Pointer,
  ) => number;
  DeleteProcThreadAttributeList: (list: Pointer) => void;
};

let kernel32: Kernel32 | null = null;
let pointerOf: ((view: Uint8Array) => number) | null = null;

/** kernel32, opened on first use so importing this module costs nothing. */
async function load(): Promise<Kernel32> {
  if (kernel32 !== null) return kernel32;
  const ffi = await import("bun:ffi");
  const library = ffi.dlopen("kernel32.dll", {
    CreateProcessW: {
      args: [
        "ptr",
        "ptr",
        "ptr",
        "ptr",
        "i32",
        "u32",
        "ptr",
        "ptr",
        "ptr",
        "ptr",
      ],
      returns: "i32",
    },
    CreateFileW: {
      args: ["ptr", "u32", "u32", "ptr", "u32", "u32", "u64"],
      returns: "u64",
    },
    CloseHandle: { args: ["u64"], returns: "i32" },
    WaitForSingleObject: { args: ["u64", "u32"], returns: "u32" },
    GetExitCodeProcess: { args: ["u64", "ptr"], returns: "i32" },
    TerminateProcess: { args: ["u64", "u32"], returns: "i32" },
    GetLastError: { args: [], returns: "u32" },
    InitializeProcThreadAttributeList: {
      args: ["ptr", "u32", "u32", "ptr"],
      returns: "i32",
    },
    UpdateProcThreadAttribute: {
      args: ["ptr", "u32", "u64", "ptr", "u64", "ptr", "ptr"],
      returns: "i32",
    },
    DeleteProcThreadAttributeList: { args: ["ptr"], returns: "void" },
  });
  pointerOf = (view) => ffi.ptr(view) as unknown as number;
  kernel32 = library.symbols as unknown as Kernel32;
  return kernel32;
}

function wide(text: string): Uint8Array {
  return Buffer.from(`${text}\0`, "utf16le");
}

/** Keeps buffers that Windows still points into reachable until here. */
function retain(..._values: unknown[]): void {}

function win32Error(call: string, k: Kernel32): Error {
  return new Error(`${call} failed with Windows error ${k.GetLastError()}`);
}

/** One argument quoted exactly as subprocess.list2cmdline quoted it. */
export function quoteArgument(argument: string): string {
  const needQuote =
    argument === "" || argument.includes(" ") || argument.includes("\t");
  let quoted = needQuote ? '"' : "";
  let backslashes = 0;
  for (const character of argument) {
    if (character === "\\") {
      backslashes += 1;
    } else if (character === '"') {
      quoted += `${"\\".repeat(backslashes * 2)}\\"`;
      backslashes = 0;
    } else {
      quoted += "\\".repeat(backslashes) + character;
      backslashes = 0;
    }
  }
  quoted += "\\".repeat(backslashes);
  if (needQuote) quoted += `${"\\".repeat(backslashes)}"`;
  return quoted;
}

export function commandLine(argv: string[]): string {
  return argv.map(quoteArgument).join(" ");
}

/**
 * A CREATE_UNICODE_ENVIRONMENT block: `NAME=value` strings sorted without
 * regard to case, as Windows expects, each NUL-terminated, then one more NUL.
 */
export function environmentBlock(
  environment: Record<string, string>,
): Uint8Array {
  const names = Object.keys(environment).sort((a, b) => {
    const x = a.toUpperCase();
    const y = b.toUpperCase();
    return x < y ? -1 : x > y ? 1 : 0;
  });
  const entries = names.map((name) => `${name}=${environment[name]}\0`);
  return Buffer.from(`${entries.join("")}\0`, "utf16le");
}

/** Where a redirected child's output goes. */
export interface Redirect {
  /** The file that receives standard output, created or truncated. */
  stdout: string;
  /** The file that receives standard error; the same path shares a handle. */
  stderr: string;
}

export interface SpawnOptions {
  env: Record<string, string>;
  creationFlags: number;
  /** Omitted, the child is started with no redirected handles at all. */
  redirect?: Redirect;
}

/** A Windows child process started through CreateProcessW. */
export class Win32Process implements ChildProcess {
  readonly exited: Promise<number>;
  readonly signalCode = null;
  #exitCode: number | null = null;
  #handle: bigint | null;
  #resolve: (code: number) => void = () => {};
  #timer: ReturnType<typeof setInterval>;

  constructor(
    private readonly k: Kernel32,
    handle: bigint,
    readonly pid: number,
  ) {
    this.#handle = handle;
    this.exited = new Promise((resolve) => {
      this.#resolve = resolve;
    });
    this.#timer = setInterval(() => this.#poll(), POLL_MILLISECONDS);
  }

  /** Popen.poll(): read on access, so a check never sees a stale answer. */
  get exitCode(): number | null {
    this.#poll();
    return this.#exitCode;
  }

  #poll(): void {
    const handle = this.#handle;
    if (handle === null) return;
    if (this.k.WaitForSingleObject(handle, 0) !== WAIT_OBJECT_0) return;
    const code = new Uint32Array(1);
    this.k.GetExitCodeProcess(handle, code);
    const exitCode = code[0] ?? 0;
    this.#exitCode = exitCode;
    this.#handle = null;
    clearInterval(this.#timer);
    this.k.CloseHandle(handle);
    this.#resolve(exitCode);
  }

  /** Popen.terminate() and kill() alike: TerminateProcess with exit code 1. */
  kill(_signal?: number | NodeJS.Signals): void {
    const handle = this.#handle;
    if (handle === null) return;
    this.k.TerminateProcess(handle, 1);
    this.#poll();
  }
}

function openInheritable(
  k: Kernel32,
  path: string,
  access: number,
  disposition: number,
): bigint {
  const attributes = Buffer.alloc(SECURITY_ATTRIBUTES_SIZE);
  attributes.writeUInt32LE(SECURITY_ATTRIBUTES_SIZE, 0);
  attributes.writeUInt32LE(1, 16); // bInheritHandle
  const handle = k.CreateFileW(
    wide(path),
    access,
    FILE_SHARE_ALL,
    attributes,
    disposition,
    FILE_ATTRIBUTE_NORMAL,
    0n,
  );
  if (handle === INVALID_HANDLE_VALUE) {
    throw win32Error(`CreateFileW(${path})`, k);
  }
  return handle;
}

/**
 * Starts `argv` with CreateProcessW and the given creation flags.
 *
 * Without `redirect` the child gets no STARTF_USESTDHANDLES and inherits no
 * handle, exactly as Popen(creationflags=...) without stdio arguments. With
 * it, standard output and error go to the named files and standard input is
 * NUL, and those are the only handles the child inherits.
 */
export async function spawnWithFlags(
  argv: string[],
  options: SpawnOptions,
): Promise<Win32Process> {
  const k = await load();
  const application = argv[0];
  if (application === undefined) throw new Error("no program to start");

  const inherited: bigint[] = [];
  let attributeList: Uint8Array | null = null;
  // UpdateProcThreadAttribute keeps a pointer to this list of handles, so it
  // has to outlive CreateProcessW.
  let handles: BigUint64Array | null = null;
  try {
    let startupInfo: Buffer;
    let flags = options.creationFlags | CREATE_UNICODE_ENVIRONMENT;
    let inheritHandles = 0;
    if (options.redirect === undefined) {
      startupInfo = Buffer.alloc(STARTUPINFOW_SIZE);
      startupInfo.writeUInt32LE(STARTUPINFOW_SIZE, 0);
    } else {
      const stdoutPath = resolve(options.redirect.stdout);
      const stderrPath = resolve(options.redirect.stderr);
      const stdin = openInheritable(k, "NUL", GENERIC_READ, OPEN_EXISTING);
      inherited.push(stdin);
      const stdout = openInheritable(
        k,
        stdoutPath,
        GENERIC_WRITE,
        CREATE_ALWAYS,
      );
      inherited.push(stdout);
      let stderr = stdout;
      if (stderrPath.toLowerCase() !== stdoutPath.toLowerCase()) {
        stderr = openInheritable(k, stderrPath, GENERIC_WRITE, CREATE_ALWAYS);
        inherited.push(stderr);
      }

      const size = new BigUint64Array(1);
      k.InitializeProcThreadAttributeList(null, 1, 0, size);
      const list = new Uint8Array(Number(size[0]));
      if (!k.InitializeProcThreadAttributeList(list, 1, 0, size)) {
        throw win32Error("InitializeProcThreadAttributeList", k);
      }
      attributeList = list;
      handles = new BigUint64Array(inherited);
      const updated = k.UpdateProcThreadAttribute(
        list,
        0,
        PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
        handles,
        BigInt(handles.byteLength),
        null,
        null,
      );
      if (!updated) throw win32Error("UpdateProcThreadAttribute", k);

      startupInfo = Buffer.alloc(STARTUPINFOEXW_SIZE);
      startupInfo.writeUInt32LE(STARTUPINFOEXW_SIZE, 0);
      startupInfo.writeUInt32LE(STARTF_USESTDHANDLES, 60); // dwFlags
      startupInfo.writeBigUInt64LE(stdin, 80);
      startupInfo.writeBigUInt64LE(stdout, 88);
      startupInfo.writeBigUInt64LE(stderr, 96);
      const address = pointerOf === null ? 0 : pointerOf(list);
      startupInfo.writeBigUInt64LE(BigInt(address), 104); // lpAttributeList
      flags |= EXTENDED_STARTUPINFO_PRESENT;
      inheritHandles = 1;
    }

    const information = Buffer.alloc(PROCESS_INFORMATION_SIZE);
    const created = k.CreateProcessW(
      wide(application),
      wide(commandLine(argv)),
      null,
      null,
      inheritHandles,
      flags,
      environmentBlock(options.env),
      null,
      startupInfo,
      information,
    );
    if (!created) throw win32Error(`CreateProcessW(${application})`, k);
    k.CloseHandle(information.readBigUInt64LE(8)); // hThread
    return new Win32Process(
      k,
      information.readBigUInt64LE(0),
      information.readUInt32LE(16),
    );
  } finally {
    // The child holds its own copies; these are closed as Popen closed its.
    if (attributeList !== null) k.DeleteProcThreadAttributeList(attributeList);
    retain(handles);
    for (const handle of inherited) k.CloseHandle(handle);
  }
}
