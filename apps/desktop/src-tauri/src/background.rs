//! Where command bodies run.
//!
//! Tauri runs a plain `fn` command on the main thread, inside WebKit's
//! message callback. Until it returns the window can neither repaint nor take
//! input, and a panic there cannot unwind through the callback, so it aborts
//! the whole app. Every command that touches the disk, the database or another
//! process is therefore an `async fn` whose body runs here instead: on Tauri's
//! blocking pool, with a panic turned into an ordinary error.
//!
//! The main thread also used to put every command in a line for free. Changes
//! keep that order: [`change`] waits its turn at one gate, so two changes
//! never run at once and they run in the order they were asked for, as
//! before. That keeps a command of several steps (a restore, a duplicate, a
//! bulk toggle) whole, and keeps in-process state that assumes one change at
//! a time (the bundle progress, the order of a change set's parts) correct.
//! The services' own checks (the resource coordinator, the instance lock, the
//! journal and the profile revision) are unchanged and still decide whether a
//! change may go ahead. Reads take no turn ([`run`]): they already ran beside
//! the bundle import and the command line, which changes the same data from
//! another process, and while a long change runs they are what keeps the
//! window showing progress.

use crate::ipc::{self, IpcResult};
use crate::state::AppState;
use std::future::Future;
use std::panic::{catch_unwind, AssertUnwindSafe};
use tauri::{AppHandle, Manager, Runtime, State};

// Containing a panic needs it to unwind. Built with `panic = "abort"`, one
// bug in any command would close the app again.
#[cfg(not(panic = "unwind"))]
compile_error!("commands contain their panics by unwinding; build with panic = \"unwind\"");

/// Runs a command body on the blocking pool without waiting for a turn: for
/// reads, and for writes outside the manager's data (a file the user saves).
pub async fn run<R, T, F>(app: AppHandle<R>, body: F) -> IpcResult<T>
where
    R: Runtime,
    T: Send + 'static,
    F: FnOnce(AppHandle<R>, State<'_, AppState>) -> IpcResult<T> + Send + 'static,
{
    on_blocking_pool(app, None, body).await
}

/// Runs a command body that changes the manager's data on the blocking pool,
/// after every change asked for before it has finished.
pub async fn change<R, T, F>(app: AppHandle<R>, body: F) -> IpcResult<T>
where
    R: Runtime,
    T: Send + 'static,
    F: FnOnce(AppHandle<R>, State<'_, AppState>) -> IpcResult<T> + Send + 'static,
{
    let turn = app.state::<AppState>().changes.clone().lock_owned().await;
    on_blocking_pool(app, Some(turn), body).await
}

/// `change` for a body that is itself asynchronous (SMAPI setup, which
/// downloads). It runs as its own task, so a panic in it ends that task only.
pub async fn change_async<R, T, Fut>(app: AppHandle<R>, body: Fut) -> IpcResult<T>
where
    R: Runtime,
    T: Send + 'static,
    Fut: Future<Output = IpcResult<T>> + Send + 'static,
{
    let turn = app.state::<AppState>().changes.clone().lock_owned().await;
    contained_task(async move {
        let outcome = body.await;
        drop(turn);
        outcome
    })
    .await
}

/// Runs an asynchronous command body as its own task, so a panic in it
/// becomes an error instead of a promise that never settles.
pub async fn contained_task<T, Fut>(body: Fut) -> IpcResult<T>
where
    T: Send + 'static,
    Fut: Future<Output = IpcResult<T>> + Send + 'static,
{
    match tauri::async_runtime::spawn(body).await {
        Ok(outcome) => outcome,
        Err(error) => Err(stopped(error).into()),
    }
}

async fn on_blocking_pool<R, T, F>(
    app: AppHandle<R>,
    turn: Option<tokio::sync::OwnedMutexGuard<()>>,
    body: F,
) -> IpcResult<T>
where
    R: Runtime,
    T: Send + 'static,
    F: FnOnce(AppHandle<R>, State<'_, AppState>) -> IpcResult<T> + Send + 'static,
{
    let task = tauri::async_runtime::spawn_blocking(move || {
        // The turn is held by the body, not by the awaiting command, so it
        // lasts exactly as long as the work does.
        let _turn = turn;
        let handle = app.clone();
        contained(move || body(handle, app.state::<AppState>()))
    });
    match task.await {
        Ok(outcome) => outcome,
        Err(error) => Err(stopped(error).into()),
    }
}

/// Runs `body`, turning a panic into an error. Nothing it held is lost: locks
/// and leases are released as the panic unwinds, a database transaction rolls
/// back, and an operation it had started is left for recovery to settle,
/// exactly as if the app had stopped there.
pub fn contained<T>(body: impl FnOnce() -> IpcResult<T>) -> IpcResult<T> {
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(outcome) => outcome,
        Err(payload) => Err(ipc::command_panicked(panic_text(payload.as_ref())).into()),
    }
}

fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_string()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "a panic without a message".to_string()
    }
}

/// The task was cancelled (the app is closing) or panicked outside `contained`.
fn stopped(error: tauri::Error) -> manager_app::error::AppError {
    let text = error.to_string();
    ipc::command_panicked(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panic_becomes_an_internal_error_with_its_text() {
        let error = contained::<()>(|| panic!("index out of bounds: the len is 0")).unwrap_err();

        assert_eq!(error.code, ipc::COMMAND_PANICKED);
        assert_eq!(
            error.category,
            manager_app::error::AppErrorCategory::Internal
        );
        assert!(error
            .technical_details
            .as_deref()
            .is_some_and(|details| details.contains("index out of bounds: the len is 0")));
    }

    #[test]
    fn a_formatted_panic_keeps_its_text() {
        let index = 3;
        let error = contained::<()>(|| panic!("no entry {index}")).unwrap_err();
        assert!(error
            .technical_details
            .as_deref()
            .is_some_and(|details| details.contains("no entry 3")));
    }

    #[test]
    fn an_ordinary_outcome_passes_through() {
        assert_eq!(contained(|| Ok(7)).unwrap(), 7);
        let refused = contained::<()>(|| Err(ipc::no_active_profile().into())).unwrap_err();
        assert_eq!(refused.code, ipc::NO_ACTIVE_PROFILE);
    }
}
