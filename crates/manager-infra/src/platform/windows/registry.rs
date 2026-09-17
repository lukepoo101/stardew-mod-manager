//! Read-only access to the Windows registry.
//!
//! Steam records its install location under the same key for both the 64-bit
//! and 32-bit views, so a WOW64-aware read is required on a 64-bit host: a
//! 32-bit process would otherwise see only the redirected view. Nothing here
//! writes to the registry.

#![allow(non_snake_case, clippy::upper_case_acronyms)]

use std::ffi::c_void;

type HKEY = *mut c_void;
type LSTATUS = i32;
type REGSAM = u32;

pub const HKEY_CURRENT_USER: HKEY = 0x8000_0001usize as HKEY;
pub const HKEY_LOCAL_MACHINE: HKEY = 0x8000_0002usize as HKEY;

pub const KEY_READ: REGSAM = 0x0002_0000;
pub const KEY_WOW64_64KEY: REGSAM = 0x0000_0100;

pub const RRF_RT_REG_SZ: u32 = 0x0000_0002;
pub const RRF_RT_REG_EXPAND_SZ: u32 = 0x0000_0004;

extern "system" {
    fn RegGetValueW(
        hkey: HKEY,
        lpSubKey: *const u16,
        lpValue: *const u16,
        dwFlags: u32,
        pdwType: *mut u32,
        pvData: *mut c_void,
        pcbData: *mut u32,
    ) -> LSTATUS;
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Reads a string value, expanding environment references when Windows asks.
///
/// The handle parameter is always one of the predefined root keys declared in
/// this module - never a handle that came from anywhere else - so the function
/// stays safe to call.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn read_string(root: HKEY, subkey: &str, value: &str) -> Option<String> {
    let subkey = wide(subkey);
    let value = wide(value);

    for view in [KEY_READ | KEY_WOW64_64KEY, KEY_READ] {
        let mut size: u32 = 0;
        let mut kind: u32 = 0;
        let status = unsafe {
            RegGetValueW(
                root,
                subkey.as_ptr(),
                value.as_ptr(),
                view | RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ,
                &mut kind,
                std::ptr::null_mut(),
                &mut size,
            )
        };
        if status != 0 || size == 0 {
            continue;
        }

        let mut buffer = vec![0u16; (size as usize / 2) + 1];
        let status = unsafe {
            RegGetValueW(
                root,
                subkey.as_ptr(),
                value.as_ptr(),
                view | RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ,
                &mut kind,
                buffer.as_mut_ptr() as *mut c_void,
                &mut size,
            )
        };
        if status != 0 {
            continue;
        }

        let end = buffer
            .iter()
            .position(|character| *character == 0)
            .unwrap_or(buffer.len());
        let rendered = String::from_utf16_lossy(&buffer[..end]);
        let rendered = rendered.trim().to_string();
        if rendered.is_empty() {
            continue;
        }
        // REG_EXPAND_SZ values may still contain environment references.
        return Some(
            std::env::var("SystemRoot")
                .ok()
                .filter(|_| rendered.contains("%SystemRoot%"))
                .map(|system_root| rendered.replace("%SystemRoot%", &system_root))
                .unwrap_or(rendered),
        );
    }

    None
}

/// Steam's recorded installation path, from either hive or the 32-bit view.
pub fn steam_install_path() -> Option<String> {
    const SUBKEY: &str = "SOFTWARE\\Valve\\Steam";
    for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        for value in ["SteamPath", "InstallPath"] {
            if let Some(path) = read_string(root, SUBKEY, value) {
                return Some(path.replace('/', "\\"));
            }
        }
    }
    None
}
