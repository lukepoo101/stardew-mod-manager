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

/// Runs a state-changing command body and emits the invalidation hint
/// afterwards, whether the body succeeded or failed.
///
/// A failed command is not evidence that nothing changed: a durable operation
/// can commit filesystem or database state and still return an error such as
/// `RecoveryRequired`. Emitting on both outcomes is therefore the conservative
/// choice, and a redundant invalidation is harmless.
pub fn after_state_change<R: Runtime, T>(app: &AppHandle<R>, body: impl FnOnce() -> T) -> T {
    let outcome = body();
    emit_backend_state_changed(app);
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_backend_state_changed_event_name_is_the_frontend_contract() {
        assert_eq!(BACKEND_STATE_CHANGED, "backend-state-changed");
    }
}
