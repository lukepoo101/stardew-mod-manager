//! The minimal Win32 surface the process backend needs.
//!
//! The declarations are written out rather than pulled from a binding crate so
//! the security-relevant calls are explicit and reviewable: every handle that
//! is opened here is closed by the owning type, and no call is made against a
//! pid whose incarnation has not been established.
#![allow(non_snake_case, non_camel_case_types, clippy::upper_case_acronyms)]
// Every handle parameter below comes from OwnedHandle, which only ever holds a
// handle returned by this module or by the standard library's Child, so the raw
// pointer never dangles.
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use std::ffi::c_void;

pub type HANDLE = *mut c_void;
pub type BOOL = i32;
pub type DWORD = u32;
pub type WCHAR = u16;
pub type LPVOID = *mut c_void;

pub const INVALID_HANDLE_VALUE: HANDLE = !0usize as HANDLE;

pub const TH32CS_SNAPPROCESS: DWORD = 0x0000_0002;
pub const MAX_PATH_CHARS: usize = 32_768;

pub const PROCESS_TERMINATE: DWORD = 0x0001;
pub const PROCESS_QUERY_LIMITED_INFORMATION: DWORD = 0x1000;
pub const SYNCHRONIZE: DWORD = 0x0010_0000;

pub const WAIT_OBJECT_0: DWORD = 0x0000_0000;

/// `OpenProcess` refused the requested rights against a process that exists.
pub const ERROR_ACCESS_DENIED: DWORD = 5;
/// `OpenProcess` was given a pid that is not in the process table.
pub const ERROR_INVALID_PARAMETER: DWORD = 87;

#[repr(C)]
pub struct PROCESSENTRY32W {
    pub dwSize: DWORD,
    pub cntUsage: DWORD,
    pub th32ProcessID: DWORD,
    pub th32DefaultHeapID: usize,
    pub th32ModuleID: DWORD,
    pub cntThreads: DWORD,
    pub th32ParentProcessID: DWORD,
    pub pcPriClassBase: i32,
    pub dwFlags: DWORD,
    pub szExeFile: [WCHAR; 260],
}

impl Default for PROCESSENTRY32W {
    fn default() -> Self {
        Self {
            dwSize: std::mem::size_of::<Self>() as DWORD,
            cntUsage: 0,
            th32ProcessID: 0,
            th32DefaultHeapID: 0,
            th32ModuleID: 0,
            cntThreads: 0,
            th32ParentProcessID: 0,
            pcPriClassBase: 0,
            dwFlags: 0,
            szExeFile: [0; 260],
        }
    }
}

#[repr(C)]
pub struct FILETIME {
    pub dwLowDateTime: DWORD,
    pub dwHighDateTime: DWORD,
}

impl FILETIME {
    /// The 64-bit timestamp as a single comparable value.
    pub fn as_u64(&self) -> u64 {
        ((self.dwHighDateTime as u64) << 32) | self.dwLowDateTime as u64
    }
}

extern "system" {
    pub fn CreateToolhelp32Snapshot(dwFlags: DWORD, th32ProcessID: DWORD) -> HANDLE;
    pub fn Process32FirstW(hSnapshot: HANDLE, lppe: *mut PROCESSENTRY32W) -> BOOL;
    pub fn Process32NextW(hSnapshot: HANDLE, lppe: *mut PROCESSENTRY32W) -> BOOL;
    pub fn OpenProcess(dwDesiredAccess: DWORD, bInheritHandle: BOOL, dwProcessId: DWORD) -> HANDLE;
    pub fn CloseHandle(hObject: HANDLE) -> BOOL;
    pub fn GetProcessTimes(
        hProcess: HANDLE,
        lpCreationTime: *mut FILETIME,
        lpExitTime: *mut FILETIME,
        lpKernelTime: *mut FILETIME,
        lpUserTime: *mut FILETIME,
    ) -> BOOL;
    pub fn QueryFullProcessImageNameW(
        hProcess: HANDLE,
        dwFlags: DWORD,
        lpExeName: *mut WCHAR,
        lpdwSize: *mut DWORD,
    ) -> BOOL;
    pub fn WaitForSingleObject(hHandle: HANDLE, dwMilliseconds: DWORD) -> DWORD;
    pub fn TerminateProcess(hProcess: HANDLE, uExitCode: u32) -> BOOL;
    pub fn GetLastError() -> DWORD;
}

/// Owns a Win32 handle and closes it exactly once.
#[derive(Debug)]
pub struct OwnedHandle(pub HANDLE);

impl OwnedHandle {
    pub fn is_valid(&self) -> bool {
        !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if self.is_valid() {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

// A process handle is a kernel object reference, not a pointer into thread
// local state, so moving one between threads is safe.
unsafe impl Send for OwnedHandle {}
unsafe impl Sync for OwnedHandle {}

/// The process creation timestamp, or None when the process cannot be queried.
pub fn process_creation_time(handle: HANDLE) -> Option<u64> {
    let mut creation = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let mut exit = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let mut kernel = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let mut user = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let ok = unsafe { GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) };
    (ok != 0).then(|| creation.as_u64())
}

/// The fully qualified image path of a process.
pub fn process_image_path(handle: HANDLE) -> Option<String> {
    let mut buffer = vec![0u16; MAX_PATH_CHARS];
    let mut size = buffer.len() as DWORD;
    let ok = unsafe { QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut size) };
    if ok == 0 || size == 0 {
        return None;
    }
    buffer.truncate(size as usize);
    Some(String::from_utf16_lossy(&buffer))
}

/// The image file name, without the directory.
pub fn image_file_name(image_path: &str) -> &str {
    image_path.rsplit(['\\', '/']).next().unwrap_or(image_path)
}

/// Whether the process has terminated.
pub fn has_exited(handle: HANDLE) -> bool {
    unsafe { WaitForSingleObject(handle, 0) == WAIT_OBJECT_0 }
}

/// Opens a process for observation only.
///
/// Windows fails OpenProcess when any requested right is denied by the process
/// security descriptor, and query and termination rights are separate. Asking
/// for termination rights while merely observing a game would make a process
/// that cannot be killed invisible to the manager, which is the wrong failure
/// mode for the checks that protect the game directory.
pub fn open_process_for_query(pid: u32) -> Option<OwnedHandle> {
    open(pid, PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE)
}

/// Whether a process id currently exists in the OS process table.
///
/// Existence and identity are separate questions, and this answers only the
/// first: a pid that is gone proves the session it belonged to has ended, even
/// when the incarnation can no longer be checked. `OpenProcess` answers the
/// question for free when it succeeds, and its failure reason distinguishes the
/// cases that matter - `ERROR_ACCESS_DENIED` means the process is there and
/// merely refuses the requested rights, while `ERROR_INVALID_PARAMETER` is the
/// documented "no such process". Only the denial is conclusive on its own, so
/// everything else is confirmed against the process table: reporting a live
/// process as vanished is what lets a caller start a second game instance and
/// rewrite the game directory underneath it.
pub fn pid_exists(pid: u32) -> bool {
    if open_process_for_query(pid).is_some() {
        return true;
    }
    if unsafe { GetLastError() } == ERROR_ACCESS_DENIED {
        return true;
    }
    // ERROR_INVALID_PARAMETER says "gone" and any other failure says nothing, so
    // both are settled by the snapshot, which lists the pids that exist now.
    snapshot_processes()
        .iter()
        .any(|(process, _)| *process == pid)
}

/// Opens a process this manager owns so it can be terminated.
///
/// Only called for a process whose identity has already been established, so
/// requesting PROCESS_TERMINATE here does not widen what the manager will act on.
pub fn open_owned_process_for_termination(pid: u32) -> Option<OwnedHandle> {
    open(
        pid,
        PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE,
    )
}

fn open(pid: u32, access: DWORD) -> Option<OwnedHandle> {
    let handle = unsafe { OpenProcess(access, 0, pid) };
    let owned = OwnedHandle(handle);
    if owned.is_valid() {
        Some(owned)
    } else {
        None
    }
}

/// Every running process id with its image file name.
pub fn snapshot_processes() -> Vec<(u32, String)> {
    let snapshot = OwnedHandle(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) });
    if !snapshot.is_valid() {
        return Vec::new();
    }

    let mut processes = Vec::new();
    let mut entry = PROCESSENTRY32W::default();
    let mut ok = unsafe { Process32FirstW(snapshot.0, &mut entry) };
    while ok != 0 {
        let end = entry
            .szExeFile
            .iter()
            .position(|character| *character == 0)
            .unwrap_or(entry.szExeFile.len());
        processes.push((
            entry.th32ProcessID,
            String::from_utf16_lossy(&entry.szExeFile[..end]),
        ));
        entry = PROCESSENTRY32W::default();
        ok = unsafe { Process32NextW(snapshot.0, &mut entry) };
    }
    processes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_declared_error_codes_match_the_win32_contract() {
        // These two values decide whether an unopenable pid reads as "gone" or
        // as "there but restricted", so a wrong one silently turns a live
        // process into a vanished one.
        assert_eq!(ERROR_ACCESS_DENIED, 5);
        assert_eq!(ERROR_INVALID_PARAMETER, 87);
    }

    #[test]
    fn a_live_pid_exists_and_an_unused_pid_does_not() {
        assert!(pid_exists(std::process::id()));
        assert!(!pid_exists(u32::MAX));
    }
}
