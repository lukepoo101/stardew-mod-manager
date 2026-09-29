//! How the SMAPI installer must be started so it can run to completion.
//!
//! The installer is a .NET console application that writes its own console
//! title and calls Console.Clear() unconditionally. Those APIs need a console
//! attached to the process, not merely somewhere to write: a child started with
//! redirected stdout, or with the redirect flags but no console, fails with
//! "The handle is invalid" partway through the install.
//!
//! Measured on Windows 11 with the pinned SMAPI 4.1.10 installer, a child
//! started from a process that has no console of its own:
//!
//! | start mode                                    | Console.Clear() |
//! | ---------------------------------------------- | --------------- |
//! | default (inherited, output redirected)         | fails           |
//! | CREATE_NEW_CONSOLE                             | fails           |
//! | CREATE_NEW_CONSOLE with CONOUT$ standard handles | fails         |
//! | CREATE_NEW_CONSOLE without redirected output   | succeeds        |
//!
//! The manager therefore gives the installer its own console and does not
//! redirect its output. Success is decided from the exit status and from the
//! artifacts on disk, which is the stronger evidence anyway; the installer's
//! console text is diagnostic only and is no longer parsed.
//!
//! A consequence worth stating plainly: the installer gets a real console
//! window for the duration of the install. Windows ignores CREATE_NO_WINDOW
//! when it is combined with CREATE_NEW_CONSOLE, so there is no way to have the
//! console the installer requires without it existing. The installer clears its
//! own screen and is short-lived, and this matches what running SMAPI's own
//! installer does, so the window is accepted rather than hidden.

use std::process::Command;

/// Creates the installer process so that it has a console to run in.
///
/// On every platform other than Windows this is the default, because the
/// installer does not need a console there: the Unix installer skips the
/// interactive theme step entirely.
pub fn prepare_installer_command(command: &mut Command) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;

        const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;

        // Only CREATE_NEW_CONSOLE is requested. Adding CREATE_NO_WINDOW would
        // change nothing: Windows ignores it when CREATE_NEW_CONSOLE is set, so
        // the console window appears either way and the code should not imply
        // otherwise.
        command.creation_flags(CREATE_NEW_CONSOLE);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = command;
    }
}

/// Whether the installer's console output can be captured and parsed.
///
/// It cannot on Windows, because capturing it is exactly what removes the
/// console the installer needs. Callers must not require a success message
/// there, and must rely on the exit status and the installed artifacts.
pub const OUTPUT_IS_CAPTURED: bool = !cfg!(target_os = "windows");
