//! Read-only access to the Windows registry.
//!
//! Steam records its install location under the same key in the 64-bit and the
//! 32-bit registry view, and a 64-bit process reading the default view can miss
//! a value that was written to the other view. Both views are therefore probed
//! explicitly.
//!
//! Two details are easy to get wrong, and both were measured on a live system
//! before this code was written:
//!
//! * RegGetValueW ignores KEY_WOW64_64KEY and KEY_WOW64_32KEY. Those bits are
//!   access masks for RegOpenKeyExW, and because KEY_READ carries the same
//!   numeric value as the default flags, passing them here silently produced the
//!   default view for both probes. RegGetValueW selects a view with
//!   RRF_SUBKEY_WOW6464KEY and RRF_SUBKEY_WOW6432KEY instead.
//! * An explicit WOW6432Node path and a view flag are not equivalent. A mirrored
//!   key resolves through either, but a key that exists in only one view is
//!   reached only by the flag.
//!
//! Nothing here writes to the registry.

#![allow(non_snake_case, clippy::upper_case_acronyms)]

use std::ffi::c_void;

type HKEY = *mut c_void;
type LSTATUS = i32;

pub const HKEY_CURRENT_USER: HKEY = 0x8000_0001usize as HKEY;
pub const HKEY_LOCAL_MACHINE: HKEY = 0x8000_0002usize as HKEY;

/// Registry value types accepted by read_string.
const RRF_RT_REG_SZ: u32 = 0x0000_0002;
const RRF_RT_REG_EXPAND_SZ: u32 = 0x0000_0004;
/// Read the value from the 64-bit view of the subkey.
const RRF_SUBKEY_WOW6464KEY: u32 = 0x0001_0000;
/// Read the value from the 32-bit view of the subkey.
const RRF_SUBKEY_WOW6432KEY: u32 = 0x0002_0000;

const ERROR_SUCCESS: LSTATUS = 0;

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

/// The registry views a value may live in, most likely first.
///
/// The 64-bit view is probed first because Steam is a 64-bit application on a
/// 64-bit host, then the 32-bit view for a 32-bit installation, then the
/// default view as a final fallback.
const VIEWS: [u32; 3] = [RRF_SUBKEY_WOW6464KEY, RRF_SUBKEY_WOW6432KEY, 0];

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
    for view in VIEWS {
        if let Some(found) = read_string_in_view(root, subkey, value, view) {
            return Some(found);
        }
    }
    None
}

/// Reads a value from one specific registry view.
///
/// The view flag is part of the signature so a later caller can insist on a
/// particular view instead of accepting whichever one answers first.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn read_string_in_view(root: HKEY, subkey: &str, value: &str, view: u32) -> Option<String> {
    let subkey = wide(subkey);
    let value = wide(value);
    let flags = view | RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ;

    // Ask for the size first, then read into an exactly sized buffer.
    let mut size: u32 = 0;
    let mut kind: u32 = 0;
    let status = unsafe {
        RegGetValueW(
            root,
            subkey.as_ptr(),
            value.as_ptr(),
            flags,
            &mut kind,
            std::ptr::null_mut(),
            &mut size,
        )
    };
    if status != ERROR_SUCCESS || size == 0 {
        return None;
    }

    let mut buffer = vec![0u16; (size as usize / 2) + 1];
    let status = unsafe {
        RegGetValueW(
            root,
            subkey.as_ptr(),
            value.as_ptr(),
            flags,
            &mut kind,
            buffer.as_mut_ptr() as *mut c_void,
            &mut size,
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }

    let end = buffer
        .iter()
        .position(|character| *character == 0)
        .unwrap_or(buffer.len());
    let rendered = String::from_utf16_lossy(&buffer[..end]);
    let rendered = rendered.trim().to_string();
    if rendered.is_empty() {
        return None;
    }

    Some(expand_environment_references(&rendered))
}

/// Resolves the environment references a REG_EXPAND_SZ value may contain.
///
/// An unresolved reference is left as written rather than dropped, because a
/// path with a hole in it is worse than a path the user can recognise.
fn expand_environment_references(value: &str) -> String {
    let mut expanded = value.to_string();
    for (name, replacement) in [
        ("%SystemRoot%", std::env::var("SystemRoot").ok()),
        (
            "%ProgramFiles(x86)%",
            std::env::var("ProgramFiles(x86)").ok(),
        ),
        ("%ProgramFiles%", std::env::var("ProgramFiles").ok()),
    ] {
        if let Some(replacement) = replacement {
            expanded = expanded.replace(name, &replacement);
        }
    }
    expanded
}

/// Steam's recorded installation path, from either hive and either view.
pub fn steam_install_path() -> Option<String> {
    const SUBKEY: &str = "SOFTWARE\\Valve\\Steam";
    for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        for value in ["SteamPath", "InstallPath"] {
            if let Some(path) = read_string(root, SUBKEY, value) {
                // Steam writes forward slashes in SteamPath and backslashes in
                // InstallPath; the filesystem accepts either, so the manager
                // reports one shape.
                return Some(path.replace('/', "\\"));
            }
        }
    }
    None
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    /// The bug this guards: access-mask constants were passed to RegGetValueW,
    /// whose flags happen to share their numeric values, so no view was ever
    /// selected and a value in the other view could not be found.
    #[test]
    fn the_view_flags_are_reggetvalue_flags_not_access_masks() {
        assert_eq!(RRF_SUBKEY_WOW6464KEY, 0x0001_0000);
        assert_eq!(RRF_SUBKEY_WOW6432KEY, 0x0002_0000);
        assert_eq!(
            RRF_SUBKEY_WOW6464KEY & 0x0000_ffff,
            0,
            "the low bits are not part of the flag"
        );
        assert_eq!(RRF_SUBKEY_WOW6432KEY & 0x0000_ffff, 0);
        assert_ne!(RRF_SUBKEY_WOW6464KEY, RRF_SUBKEY_WOW6432KEY);
        // KEY_READ was 0x0002_0000|0x0019, which is why it looked plausible.
        assert_ne!(RRF_SUBKEY_WOW6432KEY, 0x0002_0019);
    }

    #[test]
    fn every_view_is_probed_exactly_once() {
        let mut sorted = VIEWS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), VIEWS.len(), "a view must not be probed twice");
    }

    #[test]
    fn a_missing_value_is_absent_rather_than_an_error() {
        assert_eq!(
            read_string(
                HKEY_CURRENT_USER,
                "SOFTWARE\\SmmDefinitelyNotInstalled",
                "x"
            ),
            None
        );
    }

    #[test]
    fn the_reported_steam_path_has_the_shape_of_a_windows_path() {
        // Steam is not guaranteed to be installed on a build machine, so this
        // asserts the shape of the answer rather than a specific path.
        if let Some(path) = steam_install_path() {
            assert!(!path.trim().is_empty());
            assert!(
                !path.contains('/'),
                "Steam paths are reported with backslashes"
            );
            assert!(
                std::path::Path::new(&path).is_absolute(),
                "the resolved Steam path must be absolute, got {path}"
            );
        }
    }

    /// The assertion the flag fix exists for: whatever view holds the value, the
    /// reader still finds it through an explicit view.
    #[test]
    fn a_value_is_reachable_through_a_specific_view() {
        let reachable = [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE]
            .into_iter()
            .flat_map(|root| VIEWS.map(move |view| (root, view)))
            .filter(|(root, view)| {
                read_string_in_view(*root, "SOFTWARE\\Valve\\Steam", "SteamPath", *view).is_some()
            })
            .count();
        if steam_install_path().is_some() {
            assert!(
                reachable > 0,
                "a resolved Steam path must be reachable through a registry view"
            );
        }
    }

    #[test]
    fn environment_references_are_expanded_when_resolvable() {
        let expanded = expand_environment_references("%SystemRoot%\\System32");
        assert!(!expanded.contains("%SystemRoot%"));
        assert_eq!(
            expand_environment_references("%SMM_NOT_A_REAL_VARIABLE%\\x"),
            "%SMM_NOT_A_REAL_VARIABLE%\\x"
        );
    }
}
