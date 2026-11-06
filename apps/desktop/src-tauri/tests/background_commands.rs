//! Commands run off the main thread, contain their panics and keep changes in
//! line.
//!
//! The commands here are test commands built on the same `background` helpers
//! every product command uses, dispatched through Tauri's real invoke path on
//! the mock runtime over the production app state. Like the other
//! invoke-handler tests they are Linux-only: linking the mock webview runtime
//! elsewhere needs a WebView2 runtime that CI only provisions for the Linux
//! quality job.
#![cfg(target_os = "linux")]
// Command results carry the rich application error, as in the library.
#![allow(clippy::result_large_err)]

use manager_app::services::resources::ResourceClaim;
use manager_core::operation::ResourceKind;
use manager_infra::paths::AppPaths;
use serde_json::{json, Value};
use stardew_mod_manager::background;
use stardew_mod_manager::ipc::{IpcResult, COMMAND_PANICKED};
use stardew_mod_manager::state::AppState;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::Manager;

/// What the line test commands record: (name, started, finished).
type Log = Arc<Mutex<Vec<(String, Instant, Instant)>>>;

#[tauri::command]
fn read_that_panics<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> IpcResult<background::Job<()>> {
    background::run(&app, |_notice, _state| {
        let empty: Vec<u8> = Vec::new();
        let index = empty.len() + 3;
        std::hint::black_box(empty[index]);
        Ok(())
    })
}

/// Takes a lease on a profile, as a toggle or an install does, then panics
/// halfway through.
#[tauri::command]
fn change_that_panics<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> IpcResult<background::Job<()>> {
    background::change(&app, |_notice, state| {
        let _lease = state
            .resources
            .try_acquire(&[ResourceClaim::write(ResourceKind::Profile, "p")])
            .expect("the profile is free");
        panic!("a bug halfway through a change");
    })
}

#[tauri::command]
fn async_change_that_panics<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> IpcResult<background::Job<()>> {
    background::change_async(&app, |_notice, _state| async {
        tokio::task::yield_now().await;
        panic!("a bug in an asynchronous change");
    })
}

#[tauri::command]
fn profile_is_free<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> IpcResult<background::Job<bool>> {
    background::run(&app, |_notice, state| Ok(state.resources.is_idle()))
}

#[tauri::command]
fn timed_change<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    name: String,
    millis: u64,
) -> IpcResult<background::Job<()>> {
    let log = app.state::<Log>().inner().clone();
    background::change(&app, move |_notice, _state| {
        let started = Instant::now();
        std::thread::sleep(Duration::from_millis(millis));
        log.lock().unwrap().push((name, started, Instant::now()));
        Ok(())
    })
}

/// How many slow reads are running, and the most there ever were.
type Running = Arc<Mutex<(usize, usize)>>;

#[tauri::command]
fn slow_read<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> IpcResult<background::Job<()>> {
    let running = app.state::<Running>().inner().clone();
    background::run(&app, move |_notice, _state| {
        {
            let mut count = running.lock().unwrap();
            count.0 += 1;
            count.1 = count.1.max(count.0);
        }
        std::thread::sleep(Duration::from_millis(100));
        running.lock().unwrap().0 -= 1;
        Ok(())
    })
}

#[tauri::command]
fn timed_read<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    name: String,
) -> IpcResult<background::Job<()>> {
    let log = app.state::<Log>().inner().clone();
    background::run(&app, move |_notice, _state| {
        let now = Instant::now();
        log.lock().unwrap().push((name, now, now));
        Ok(())
    })
}

struct World {
    window: tauri::WebviewWindow<tauri::test::MockRuntime>,
    log: Log,
    _app: tauri::App<tauri::test::MockRuntime>,
    _tmp: tempfile::TempDir,
}

fn world() -> World {
    let tmp = tempfile::tempdir().expect("temp dir");
    let paths = AppPaths::new(tmp.path().join("data"), tmp.path().join("cache"));
    let state = AppState::new_with_expected_smapi_hash(paths, Some("test")).expect("app state");
    let log: Log = Arc::default();
    let app = tauri::test::mock_builder()
        .manage(state)
        .manage(background::Jobs::new())
        .manage(log.clone())
        .manage(Running::default())
        .invoke_handler(tauri::generate_handler![
            read_that_panics,
            change_that_panics,
            async_change_that_panics,
            profile_is_free,
            timed_change,
            timed_read,
            slow_read,
        ])
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("tauri app");
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("mock window");
    World {
        window,
        log,
        _app: app,
        _tmp: tmp,
    }
}

fn try_invoke(
    window: &tauri::WebviewWindow<tauri::test::MockRuntime>,
    cmd: &str,
    body: Value,
) -> Result<Value, Value> {
    tauri::test::get_ipc_response(
        window,
        tauri::webview::InvokeRequest {
            cmd: cmd.into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: "tauri://localhost".parse().unwrap(),
            body: tauri::ipc::InvokeBody::Json(body),
            headers: Default::default(),
            invoke_key: tauri::test::INVOKE_KEY.into(),
        },
    )
    .map(|value| value.deserialize().unwrap())
    .and_then(|value| stardew_mod_manager::background::answer_of(window.app_handle(), value))
}

fn assert_panic_error(error: &Value, text: &str) {
    assert_eq!(error["code"], COMMAND_PANICKED, "{error}");
    assert_eq!(error["category"], "internal", "{error}");
    assert!(
        error["technical_details"]
            .as_str()
            .is_some_and(|details| details.contains(text)),
        "the panic's text is the technical detail: {error}"
    );
    assert!(
        error["summary"]
            .as_str()
            .is_some_and(|summary| summary.contains("keeps working")),
        "{error}"
    );
}

#[test]
fn a_panicking_command_returns_an_error_and_the_app_keeps_answering() {
    let world = world();

    let error = try_invoke(&world.window, "read_that_panics", json!({})).unwrap_err();
    assert_panic_error(&error, "index out of bounds");

    // The process is still here and still answers.
    assert_eq!(
        try_invoke(&world.window, "profile_is_free", json!({})).unwrap(),
        json!(true)
    );
}

#[test]
fn a_change_that_panics_lets_go_of_its_lease_and_its_turn() {
    let world = world();

    let error = try_invoke(&world.window, "change_that_panics", json!({})).unwrap_err();
    assert_panic_error(&error, "a bug halfway through a change");

    // The lease was released as the panic unwound, so the profile is not
    // held by a change nobody is carrying out ...
    assert_eq!(
        try_invoke(&world.window, "profile_is_free", json!({})).unwrap(),
        json!(true)
    );
    // ... and the turn was handed on: the next change runs.
    try_invoke(
        &world.window,
        "timed_change",
        json!({"name": "after", "millis": 0}),
    )
    .expect("the next change gets its turn");

    let error = try_invoke(&world.window, "async_change_that_panics", json!({})).unwrap_err();
    assert_panic_error(&error, "a bug in an asynchronous change");
    try_invoke(
        &world.window,
        "timed_change",
        json!({"name": "after async", "millis": 0}),
    )
    .expect("the next change gets its turn after an asynchronous panic too");
}

/// The main thread used to run commands one at a time. Changes still do, in
/// the order they were asked for; reads no longer wait behind them, which is
/// what lets the window show progress while a long change runs.
#[test]
fn changes_take_turns_in_order_while_reads_go_straight_through() {
    let world = world();
    let send = |cmd: &'static str, body: Value| {
        let window = world.window.clone();
        std::thread::spawn(move || try_invoke(&window, cmd, body).expect(cmd))
    };

    let long = send("timed_change", json!({"name": "long", "millis": 600}));
    std::thread::sleep(Duration::from_millis(150));
    let next = send("timed_change", json!({"name": "next", "millis": 0}));
    std::thread::sleep(Duration::from_millis(150));
    let read = send("timed_read", json!({"name": "read"}));
    for handle in [read, next, long] {
        handle.join().unwrap();
    }

    let log = world.log.lock().unwrap();
    let find = |name: &str| {
        log.iter()
            .find(|(entry, ..)| entry == name)
            .map(|(_, started, finished)| ((), *started, *finished))
            .unwrap_or_else(|| panic!("{name} ran"))
    };
    let (_, _, long_finished) = find("long");
    let (_, next_started, _) = find("next");
    let (_, read_at, _) = find("read");
    assert!(
        next_started >= long_finished,
        "a change waits for the one before it"
    );
    assert!(
        read_at < long_finished,
        "a read does not wait for a change to finish"
    );
    let order: Vec<&str> = log.iter().map(|(name, ..)| name.as_str()).collect();
    assert_eq!(order, ["read", "long", "next"]);
}

/// Closing the window used to wait for a running change, because the change
/// held the main thread. It still does: while a change runs or waits, the
/// close is held, and it goes ahead once the window has its answer.
#[test]
fn closing_the_window_waits_for_the_change_that_is_running() {
    let world = world();
    let app = world.window.app_handle().clone();
    assert!(
        !background::close_after_changes(&app),
        "with nothing running the window closes at once"
    );

    let running = {
        let window = world.window.clone();
        std::thread::spawn(move || {
            try_invoke(
                &window,
                "timed_change",
                json!({"name": "install", "millis": 500}),
            )
            .expect("the change finishes")
        })
    };
    std::thread::sleep(Duration::from_millis(150));
    assert!(
        background::close_after_changes(&app),
        "the window waits while a change runs"
    );
    assert!(app.state::<background::Jobs>().changes_waiting());
    running.join().unwrap();
    assert!(!app.state::<background::Jobs>().changes_waiting());
    assert_eq!(
        world.log.lock().unwrap().len(),
        1,
        "the change ran to its end"
    );
}

/// The window asks for a dozen reads after every change. They run a few at a
/// time, so they do not all queue for the database in front of the change
/// that is running.
#[test]
fn reads_run_a_few_at_a_time() {
    let world = world();
    let reads: Vec<_> = (0..12)
        .map(|_| {
            let window = world.window.clone();
            std::thread::spawn(move || try_invoke(&window, "slow_read", json!({})).unwrap())
        })
        .collect();
    for read in reads {
        read.join().unwrap();
    }
    let most = world
        .window
        .app_handle()
        .state::<Running>()
        .lock()
        .unwrap()
        .1;
    assert!((2..=4).contains(&most), "{most} reads ran at once");
}
