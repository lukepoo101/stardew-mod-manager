//! Where command bodies run.
//!
//! Tauri runs a plain `fn` command on the main thread, inside WebKit's
//! message callback. Until it returns the window can neither repaint nor take
//! input, and a panic there cannot unwind through the callback, so it aborts
//! the whole app. So every command that touches the disk, the database or
//! another process hands its body to a thread here and returns at once with a
//! [`Job`]; the window asks [`job_results`] for the outcome.
//!
//! Why not `async` commands: Tauri finishes an async command on a worker
//! thread and drops its handle to the window there. On Linux that handle
//! holds a reference count the GTK event loop also changes without
//! synchronisation (`Rc` in tao's window target), so many commands finishing
//! while the window works corrupted the heap and crashed the app. Here no
//! Tauri object ever leaves the main thread: a command is received, answered
//! and its window handle dropped there, and the bodies only get the app state
//! and a [`Notice`] to say they changed something.
//!
//! The main thread also used to put every command in a line for free. Changes
//! keep that order: [`change`] hands its body to the one thread that runs
//! changes, in the order they were asked for, so two changes never run at
//! once. That keeps a command of several steps (a restore, a duplicate, a bulk
//! toggle) whole, and keeps in-process state that assumes one change at a time
//! (the bundle progress, the order of a change set's parts) correct. The
//! services' own checks (the resource coordinator, the instance lock, the
//! journal and the profile revision) are unchanged and still decide whether a
//! change may go ahead. Reads take no turn ([`run`]): they already ran beside
//! the bundle import and the command line, which changes the same data from
//! another process, and while a long change runs they are what keeps the
//! window showing progress.

use crate::events;
use crate::ipc::{self, IpcResult};
use crate::state::AppState;
use manager_app::api::dto::ApiErrorDto;
use serde::Serialize;
use std::collections::HashMap;
use std::future::Future;
use std::marker::PhantomData;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use tauri::{AppHandle, Manager, Runtime};

// Containing a panic needs it to unwind. Built with `panic = "abort"`, one
// bug in any command would close the app again.
#[cfg(not(panic = "unwind"))]
compile_error!("commands contain their panics by unwinding; build with panic = \"unwind\"");

/// What a command returns at once: the job the window asks about. `T` is
/// the answer the job will have.
#[derive(Serialize)]
pub struct Job<T> {
    #[serde(rename = "__job")]
    pub job: u64,
    #[serde(skip)]
    answer: PhantomData<fn() -> T>,
}

impl<T> Job<T> {
    fn new(job: u64) -> Self {
        Self {
            job,
            answer: PhantomData,
        }
    }
}

impl<T> Clone for Job<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Job<T> {}

impl<T> std::fmt::Debug for Job<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Job").field("job", &self.job).finish()
    }
}

/// How a job ended, as [`job_results`] hands it to the window.
#[derive(Debug, Clone, Serialize)]
pub struct Finished {
    pub job: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ok: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub err: Option<ApiErrorDto>,
    #[serde(skip)]
    changed: bool,
}

/// Given to a command body in place of the app handle: it records that the
/// body may have changed something, and the window is told once the body has
/// finished (see `events::after_state_change`).
#[derive(Debug, Default)]
pub struct Notice {
    changed: AtomicBool,
}

impl Notice {
    pub fn changed(&self) {
        self.changed.store(true, Ordering::SeqCst);
    }
}

type ChangeBody = Box<dyn FnOnce() + Send>;

/// How many reads run at once.
const READ_THREADS: usize = 4;

/// The window's jobs, managed as Tauri state. The body threads share the
/// inside, which holds nothing of Tauri's.
pub struct Jobs(Arc<Inner>);

struct Inner {
    next: AtomicU64,
    finished: Mutex<HashMap<u64, Finished>>,
    changes: Mutex<mpsc::Sender<ChangeBody>>,
    reads: Mutex<mpsc::Sender<ChangeBody>>,
    /// Changes asked for and not finished yet.
    changes_waiting: AtomicUsize,
    /// The user closed the window while changes were waiting.
    closing: AtomicBool,
}

impl Default for Jobs {
    fn default() -> Self {
        Self::new()
    }
}

impl Jobs {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel::<ChangeBody>();
        let _ = std::thread::Builder::new()
            .name("changes".to_string())
            .spawn(move || {
                // One change at a time, in the order they were asked for.
                for body in receiver {
                    body();
                }
            });
        // A few reads at a time. The window asks for a dozen at once after
        // every change, and all of them at once would only queue for the
        // database and hold the change that is running up behind them.
        let (reads, queue) = mpsc::channel::<ChangeBody>();
        let queue = Arc::new(Mutex::new(queue));
        for index in 0..READ_THREADS {
            let queue = queue.clone();
            let _ = std::thread::Builder::new()
                .name(format!("reads-{index}"))
                .spawn(move || loop {
                    let next = queue
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .recv();
                    match next {
                        Ok(body) => body(),
                        Err(_) => return,
                    }
                });
        }
        Self(Arc::new(Inner {
            next: AtomicU64::new(1),
            finished: Mutex::new(HashMap::new()),
            changes: Mutex::new(sender),
            reads: Mutex::new(reads),
            changes_waiting: AtomicUsize::new(0),
            closing: AtomicBool::new(false),
        }))
    }

    /// Whether changes are running or waiting their turn.
    pub fn changes_waiting(&self) -> bool {
        self.0.changes_waiting.load(Ordering::SeqCst) > 0
    }
}

impl Inner {
    fn start(&self) -> u64 {
        self.next.fetch_add(1, Ordering::SeqCst)
    }

    fn finish(&self, job: u64, outcome: IpcResult<serde_json::Value>, notice: &Notice) {
        let (ok, err) = match outcome {
            Ok(value) => (Some(value), None),
            Err(error) => (None, Some(error)),
        };
        let finished = Finished {
            job,
            ok,
            err,
            changed: notice.changed.load(Ordering::SeqCst),
        };
        self.finished
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(job, finished);
    }
}

/// The app state and the jobs, for a body on another thread. Taken on the
/// main thread; neither holds anything of Tauri's.
fn parts<R: Runtime>(app: &AppHandle<R>) -> (AppState, Arc<Inner>) {
    let state = app.state::<AppState>().inner().clone();
    (state, app.state::<Jobs>().0.clone())
}

/// Runs a command body on one of the read threads without waiting for a
/// change: for reads, and for writes outside the manager's data (a file the
/// user saves).
pub fn run<R, T, F>(app: &AppHandle<R>, body: F) -> IpcResult<Job<T>>
where
    R: Runtime,
    T: Serialize + 'static,
    F: FnOnce(&Notice, &AppState) -> IpcResult<T> + Send + 'static,
{
    let (state, jobs) = parts(app);
    let job = jobs.start();
    let inner = jobs.clone();
    let queued = jobs
        .reads
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .send(Box::new(move || {
            let notice = Notice::default();
            let outcome = contained(|| body(&notice, &state).and_then(to_json));
            inner.finish(job, outcome, &notice);
        }));
    if queued.is_err() {
        return Err(ipc::command_panicked("the threads that run reads have stopped").into());
    }
    Ok(Job::new(job))
}

/// Runs a command body that changes the manager's data on the changes
/// thread, after every change asked for before it has finished.
pub fn change<R, T, F>(app: &AppHandle<R>, body: F) -> IpcResult<Job<T>>
where
    R: Runtime,
    T: Serialize + 'static,
    F: FnOnce(&Notice, &AppState) -> IpcResult<T> + Send + 'static,
{
    let (state, jobs) = parts(app);
    let job = jobs.start();
    jobs.changes_waiting.fetch_add(1, Ordering::SeqCst);
    let inner = jobs.clone();
    let queued = jobs
        .changes
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .send(Box::new(move || {
            let notice = Notice::default();
            let outcome = contained(|| body(&notice, &state).and_then(to_json));
            // A change stopped by a panic may have changed something before.
            if outcome
                .as_ref()
                .is_err_and(|error| error.code == ipc::COMMAND_PANICKED)
            {
                notice.changed();
            }
            inner.finish(job, outcome, &notice);
            inner.changes_waiting.fetch_sub(1, Ordering::SeqCst);
        }));
    if queued.is_err() {
        jobs.changes_waiting.fetch_sub(1, Ordering::SeqCst);
        return Err(ipc::command_panicked("the thread that runs changes has stopped").into());
    }
    Ok(Job::new(job))
}

/// `change` for a body that is itself asynchronous (SMAPI setup, which
/// downloads). It is driven to its end on the changes thread.
pub fn change_async<R, T, Fut>(
    app: &AppHandle<R>,
    body: impl FnOnce(Arc<Notice>, AppState) -> Fut + Send + 'static,
) -> IpcResult<Job<T>>
where
    R: Runtime,
    T: Serialize + Send + 'static,
    Fut: Future<Output = IpcResult<T>> + Send + 'static,
{
    change(app, move |notice, state| {
        let shared = Arc::new(Notice::default());
        let future = body(shared.clone(), state.clone());
        let outcome = tauri::async_runtime::block_on(contained_task(future));
        if shared.changed.load(Ordering::SeqCst) {
            notice.changed();
        }
        outcome
    })
}

/// Runs an asynchronous command body (a download) without a turn.
pub fn task<R, T, Fut>(
    app: &AppHandle<R>,
    body: impl FnOnce(AppState) -> Fut + Send + 'static,
) -> IpcResult<Job<T>>
where
    R: Runtime,
    T: Serialize + Send + 'static,
    Fut: Future<Output = IpcResult<T>> + Send + 'static,
{
    run(app, move |_notice, state| {
        tauri::async_runtime::block_on(contained_task(body(state.clone())))
    })
}

/// Starts a job that something else finishes (a native dialog's answer).
/// The [`Answer`] holds nothing of Tauri's, so it may be sent from anywhere;
/// dropped unsent, the job fails as a dialog that gave no answer.
pub fn pending<R: Runtime, T>(app: &AppHandle<R>) -> (Job<T>, Answer) {
    let jobs = app.state::<Jobs>().0.clone();
    let job = jobs.start();
    (
        Job::new(job),
        Answer {
            jobs,
            job,
            sent: false,
        },
    )
}

pub struct Answer {
    jobs: Arc<Inner>,
    job: u64,
    sent: bool,
}

impl Answer {
    pub fn send<T: Serialize>(mut self, value: T) {
        self.sent = true;
        self.jobs
            .finish(self.job, to_json(value), &Notice::default());
    }
}

impl Drop for Answer {
    fn drop(&mut self) {
        if !self.sent {
            self.jobs.finish(
                self.job,
                Err(ipc::native_dialog_failed("the dialog closed without an answer").into()),
                &Notice::default(),
            );
        }
    }
}

fn to_json<T: Serialize>(value: T) -> IpcResult<serde_json::Value> {
    serde_json::to_value(value).map_err(|error| {
        ipc::command_panicked(format!("the answer could not be encoded: {error}")).into()
    })
}

/// Hands finished jobs to the window. Runs on the main thread, so this is
/// also where the window hears that something changed, and where a close
/// that waited for changes goes ahead.
#[tauri::command]
pub fn job_results<R: Runtime>(app: AppHandle<R>, jobs: Vec<u64>) -> Vec<Finished> {
    take_results(&app, &jobs)
}

fn take_results<R: Runtime>(app: &AppHandle<R>, jobs: &[u64]) -> Vec<Finished> {
    let registry = app.state::<Jobs>().0.clone();
    let mut done = Vec::new();
    {
        let mut finished = registry
            .finished
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for job in jobs {
            if let Some(entry) = finished.remove(job) {
                done.push(entry);
            }
        }
    }
    if done.iter().any(|entry| entry.changed) {
        events::emit_backend_state_changed(app);
    }
    if registry.closing.load(Ordering::SeqCst)
        && registry.changes_waiting.load(Ordering::SeqCst) == 0
    {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.destroy();
        }
    }
    done
}

/// For tests that drive commands through the invoke handler: the answer of
/// a command, waiting for its job as the window would.
#[doc(hidden)]
pub fn answer_of<R: Runtime>(
    app: &AppHandle<R>,
    answer: serde_json::Value,
) -> Result<serde_json::Value, serde_json::Value> {
    let Some(job) = answer
        .as_object()
        .filter(|object| object.len() == 1)
        .and_then(|object| object.get("__job"))
        .and_then(serde_json::Value::as_u64)
    else {
        return Ok(answer);
    };
    loop {
        if let Some(finished) = take_results(app, &[job]).pop() {
            return match finished.err {
                Some(error) => Err(serde_json::to_value(error).unwrap_or_default()),
                None => Ok(finished.ok.unwrap_or_default()),
            };
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

/// Called on the main thread when the user closes the window. While a change
/// is running, or waiting its turn, the window stays open and closes once the
/// last of them has finished; returns whether it is waiting. Closing used to
/// wait by itself, because a change held the main thread; now that it does
/// not, this keeps closing from cutting an install or a toggle off halfway
/// and leaving it for recovery at the next start.
pub fn close_after_changes<R: Runtime>(app: &AppHandle<R>) -> bool {
    let Some(jobs) = app.try_state::<Jobs>() else {
        return false;
    };
    if !jobs.changes_waiting() {
        return false;
    }
    jobs.0.closing.store(true, Ordering::SeqCst);
    true
}

/// Runs an asynchronous body as its own task, so a panic in it becomes an
/// error instead of an answer that never comes.
async fn contained_task<T, Fut>(body: Fut) -> IpcResult<T>
where
    T: Send + 'static,
    Fut: Future<Output = IpcResult<T>> + Send + 'static,
{
    match tauri::async_runtime::spawn(body).await {
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

    /// The window joins a second press of a change to the one running
    /// (`JOINABLE` in `invoke.ts`) and never joins anything else, so that list
    /// has to name exactly the commands that change data here. Refreshing the
    /// launch session takes a turn but is not something a user presses.
    #[test]
    fn the_window_knows_which_commands_are_changes() {
        use std::collections::BTreeSet;
        let sources = [
            include_str!("commands.rs"),
            include_str!("operation_commands.rs"),
            include_str!("modern_smapi.rs"),
        ];
        let mut changes = BTreeSet::new();
        for source in sources {
            let mut name = None;
            for line in source.lines() {
                if let Some(rest) = line
                    .strip_prefix("pub async fn ")
                    .or_else(|| line.strip_prefix("pub fn "))
                {
                    name = rest
                        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                        .next()
                        .map(str::to_string);
                } else if line.contains("background::change(")
                    || line.contains("background::change_async(")
                {
                    changes.extend(name.clone());
                }
            }
        }
        for refresh in ["get_active_launch_session", "get_latest_launch_session"] {
            assert!(changes.remove(refresh), "{refresh} is still a change");
        }
        // Saving a file the user asked for is not a change to the manager's
        // data, but pressing Save twice would still save twice.
        changes.extend(["save_recipe_file", "save_json_export"].map(String::from));

        let invoke = include_str!("../../src/shared/api/invoke.ts");
        let list = invoke
            .split_once("const JOINABLE")
            .and_then(|(_, rest)| rest.split_once("]);"))
            .map(|(list, _)| list)
            .expect("invoke.ts lists JOINABLE");
        let joinable: BTreeSet<String> = list
            .lines()
            .filter_map(|line| line.trim().strip_prefix('"'))
            .filter_map(|line| line.split('"').next())
            .map(str::to_string)
            .collect();
        assert_eq!(joinable, changes);
    }

    #[test]
    fn an_ordinary_outcome_passes_through() {
        assert_eq!(contained(|| Ok(7)).unwrap(), 7);
        let refused = contained::<()>(|| Err(ipc::no_active_profile().into())).unwrap_err();
        assert_eq!(refused.code, ipc::NO_ACTIVE_PROFILE);
    }
}
