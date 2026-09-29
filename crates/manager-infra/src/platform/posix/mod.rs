//! The POSIX platform adapters.
//!
//! Linux and macOS share these: the Stardew Valley installation layout, the
//! SMAPI data directory and the libc process primitives are the same on both.
//! Only the Steam client locations differ, and those are data.

pub mod inspector;
pub mod log_locator;
pub mod process;
pub mod process_backend;
pub mod runtime;
pub mod steam;
