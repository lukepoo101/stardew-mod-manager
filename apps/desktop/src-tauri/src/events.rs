//! The backend cache-invalidation bridge.
//!
//! React owns presentation state; the backend owns authoritative state. Rather
//! than having every mutation hook maintain its own dependency graph of query
//! keys, the backend emits exactly one coarse, payload-free event whenever a
//! command may have changed authoritative state, and the frontend invalidates
//! its active backend queries in one place.
//!
//! This is deliberately not a domain event bus: there is one event name, no
//! payload, and no mapping from Rust events to React query keys.

use tauri::{AppHandle, Emitter, Runtime};

/// The single Tauri event that means "some authoritative backend state may have
/// changed; invalidate active backend queries".
pub const BACKEND_STATE_CHANGED: &str = "backend-state-changed";

/// Emits the coarse cache-invalidation hint.
///
/// Event delivery is a cache hint, not part of the product contract: a failed
/// emission must never convert an otherwise successful product operation into an
/// application error, so the failure is logged and dropped.
pub fn emit_backend_state_changed<R: Runtime>(app: &AppHandle<R>) {
    if let Err(error) = app.emit(BACKEND_STATE_CHANGED, ()) {
        eprintln!("failed to emit {BACKEND_STATE_CHANGED}: {error}");
    }
}

thread_local! {
    /// Set on the main thread at startup: emits the hint from there.
    static ON_MAIN_THREAD: std::cell::RefCell<Option<Box<dyn Fn()>>> =
        const { std::cell::RefCell::new(None) };
}

/// Lets [`emit_from_another_thread`] reach the window. Call on the main
/// thread.
pub fn install_main_thread_emitter<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    ON_MAIN_THREAD.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(move || emit_backend_state_changed(&app)));
    });
}

/// Emits the hint from a thread other than the main one. Emitting there
/// directly would clone and drop Tauri's window handles on that thread, whose
/// reference counts the GTK event loop changes without synchronisation; this
/// only posts a task to the main thread through the borrowed handle.
pub fn emit_from_another_thread<R: Runtime>(app: &AppHandle<R>) {
    let _ = app.run_on_main_thread(|| {
        ON_MAIN_THREAD.with(|slot| {
            if let Some(emit) = slot.borrow().as_ref() {
                emit();
            }
        });
    });
}

/// Runs a state-changing command body and emits the invalidation hint
/// afterwards, whether the body succeeded or failed.
///
/// A failed command is not evidence that nothing changed: a durable operation
/// can commit filesystem or database state and still return an error such as
/// `RecoveryRequired`. Emitting on both outcomes is therefore the conservative
/// choice, and a redundant invalidation is harmless.
pub fn after_state_change<S: StateChanged + ?Sized, T>(app: &S, body: impl FnOnce() -> T) -> T {
    let outcome = body();
    app.state_changed();
    outcome
}

/// Where "something may have changed" goes: straight to the window from the
/// main thread, or, from a command body on another thread, to its
/// [`crate::background::Notice`], which the window hears about once the body
/// has finished.
pub trait StateChanged {
    fn state_changed(&self);
}

impl<R: Runtime> StateChanged for AppHandle<R> {
    fn state_changed(&self) {
        emit_backend_state_changed(self);
    }
}

impl StateChanged for crate::background::Notice {
    fn state_changed(&self) {
        self.changed();
    }
}

impl<T: StateChanged + ?Sized> StateChanged for &T {
    fn state_changed(&self) {
        (**self).state_changed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_backend_state_changed_event_name_is_the_frontend_contract() {
        assert_eq!(BACKEND_STATE_CHANGED, "backend-state-changed");
    }
}
