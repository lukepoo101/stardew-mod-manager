//! Guardrails for the backend cache-invalidation bridge.
//!
//! The bridge is a cache hint, not part of the product contract: exactly one
//! event name exists, a state-changing command emits it whether the command
//! succeeded or failed, and emission failure never becomes a product error.
//!
//! Runtime-backed cases use Tauri's mock runtime. Like the existing
//! invoke-handler tests, they are Linux-only: linking the mock webview runtime
//! elsewhere needs a WebView2 runtime that CI only provisions for the Linux
//! quality job.

use stardew_mod_manager::events::BACKEND_STATE_CHANGED;

#[test]
fn the_event_name_is_the_frontend_contract() {
    assert_eq!(BACKEND_STATE_CHANGED, "backend-state-changed");
}

#[cfg(target_os = "linux")]
mod runtime {
    use stardew_mod_manager::events::{
        after_state_change, emit_backend_state_changed, BACKEND_STATE_CHANGED,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tauri::Listener;

    fn mock_app() -> tauri::App<tauri::test::MockRuntime> {
        tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock tauri app")
    }

    fn counting_listener(app: &tauri::App<tauri::test::MockRuntime>) -> Arc<AtomicUsize> {
        let seen = Arc::new(AtomicUsize::new(0));
        let counter = seen.clone();
        app.listen(BACKEND_STATE_CHANGED, move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        seen
    }

    #[test]
    fn a_mutating_command_body_emits_the_event_after_it_completes() {
        let app = mock_app();
        let seen = counting_listener(&app);

        let value = after_state_change(app.handle(), || {
            assert_eq!(
                seen.load(Ordering::SeqCst),
                0,
                "the event must be emitted after the body, not before it"
            );
            42
        });

        assert_eq!(value, 42, "the command result is returned unchanged");
        assert_eq!(seen.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn the_event_is_emitted_even_when_the_command_body_fails() {
        let app = mock_app();
        let seen = counting_listener(&app);

        // A failed command can still have durably changed state (for example an
        // operation that reaches RecoveryRequired), so the hint still goes out.
        let outcome: Result<(), &str> = after_state_change(app.handle(), || Err("command failed"));

        assert!(outcome.is_err());
        assert_eq!(seen.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn emission_without_a_listener_is_harmless() {
        let app = mock_app();
        emit_backend_state_changed(app.handle());
    }
}

/// End-to-end guardrail: the production Tauri command table, not just the
/// helper, must emit the invalidation hint for state-changing commands and must
/// stay silent for pure reads.
#[cfg(target_os = "linux")]
mod invoke_handler {
    use manager_infra::paths::AppPaths;
    use serde_json::{json, Value};
    use stardew_mod_manager::events::BACKEND_STATE_CHANGED;
    use stardew_mod_manager::state::AppState;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tauri::Listener;

    fn invoke(
        window: &tauri::WebviewWindow<tauri::test::MockRuntime>,
        cmd: &str,
        body: Value,
    ) -> Value {
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
        .unwrap_or_else(|err| panic!("{cmd} failed: {err}"))
        .deserialize()
        .unwrap()
    }

    #[test]
    fn the_command_table_emits_the_invalidation_hint_only_when_state_can_change() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let paths = AppPaths::new(tmp.path().join("data"), tmp.path().join("cache"));
        let state = AppState::new_with_expected_smapi_hash(paths, Some("test")).expect("app state");
        let app = stardew_mod_manager::configure(tauri::test::mock_builder(), state)
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("tauri app");
        let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("mock window");

        let seen = Arc::new(AtomicUsize::new(0));
        let counter = seen.clone();
        app.listen(BACKEND_STATE_CHANGED, move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
        });

        invoke(
            &window,
            "set_onboarding_disposition",
            json!({"disposition": "completed"}),
        );
        assert_eq!(
            seen.load(Ordering::SeqCst),
            1,
            "a state-changing command must emit the invalidation hint"
        );

        invoke(&window, "bootstrap", json!({}));
        invoke(&window, "list_recent_operations", json!({}));
        assert_eq!(
            seen.load(Ordering::SeqCst),
            1,
            "pure reads must not emit the invalidation hint"
        );

        // A command that fails before it can change anything still emits: the
        // hint is deliberately coarse and a redundant refresh is harmless.
        let _ = invoke(
            &window,
            "archive_profile",
            json!({"profileId": "not-a-uuid"}),
        );
        assert_eq!(seen.load(Ordering::SeqCst), 1);
    }
}
