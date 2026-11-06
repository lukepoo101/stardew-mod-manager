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
async fn read_that_panics<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> IpcResult<()> {
    background::run(app, |_app, _state| {
        let empty: Vec<u8> = Vec::new();
        let index = empty.len() + 3;
        std::hint::black_box(empty[index]);
        Ok(())
    })
    .await
}

/// Takes a lease on a profile, as a toggle or an install does, then panics
/// halfway through.
#[tauri::command]
async fn change_that_panics<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> IpcResult<()> {
    background::change(app, |_app, state| {
        let _lease = state
            .resources
            .try_acquire(&[ResourceClaim::write(ResourceKind::Profile, "p")])
            .expect("the profile is free");
        panic!("a bug halfway through a change");
    })
    .await
}

#[tauri::command]
async fn async_change_that_panics<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> IpcResult<()> {
    background::change_async(app, async {
        tokio::task::yield_now().await;
        panic!("a bug in an asynchronous change");
    })
    .await
}

#[tauri::command]
async fn profile_is_free<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> IpcResult<bool> {
    background::run(app, |_app, state| Ok(state.resources.is_idle())).await
}

#[tauri::command]
async fn timed_change<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    name: String,
    millis: u64,
) -> IpcResult<()> {
    background::change(app, move |app, _state| {
        let started = Instant::now();
        std::thread::sleep(Duration::from_millis(millis));
        app.state::<Log>()
            .lock()
            .unwrap()
            .push((name, started, Instant::now()));
        Ok(())
    })
    .await
}

#[tauri::command]
async fn timed_read<R: tauri::Runtime>(app: tauri::AppHandle<R>, name: String) -> IpcResult<()> {
    background::run(app, move |app, _state| {
        let now = Instant::now();
        app.state::<Log>().lock().unwrap().push((name, now, now));
        Ok(())
    })
    .await
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
        .manage(log.clone())
        .invoke_handler(tauri::generate_handler![
            read_that_panics,
            change_that_panics,
            async_change_that_panics,
            profile_is_free,
            timed_change,
            timed_read,
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
/// held the main thread. It still does: the window closes once the change has
/// finished, not halfway through it.
#[test]
fn closing_the_window_waits_for_the_change_that_is_running() {
    let world = world();
    let app = world.window.app_handle().clone();
    let closed: Log = Arc::default();
    let close = |closed: &Log| {
        let closed = closed.clone();
        move || {
            let now = Instant::now();
            closed.lock().unwrap().push(("closed".into(), now, now));
        }
    };
    assert!(
        !background::close_after_changes(&app, close(&closed)),
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
        background::close_after_changes(&app, close(&closed)),
        "the window waits while a change runs"
    );
    running.join().unwrap();

    let deadline = Instant::now() + Duration::from_secs(5);
    while closed.lock().unwrap().is_empty() {
        assert!(
            Instant::now() < deadline,
            "the window closes after the change"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let (_, _, change_finished) = world.log.lock().unwrap()[0];
    let (_, closed_at, _) = closed.lock().unwrap()[0];
    assert!(
        closed_at >= change_finished,
        "closed only once the change ended"
    );
}
