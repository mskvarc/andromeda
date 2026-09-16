//! The sanctioned baseline of tolerated detail-wording violations.
//!
//! Each entry is one [`crate::detail_message::DetailViolation::key`]: the owning
//! file, the broken rule, and the verbatim message, tab-separated. The gate in
//! [`crate::detail_message`] tolerates exactly these and fails on any new one, so
//! the list can only shrink. A change that fixes a message must delete its entry
//! here, or the gate fails the entry as stale.
//!
//! The two entries that remain both interpolate a [`std::time::Duration`], which
//! implements `Debug` but not `Display`, so `{duration:?}` is the only rendering
//! available and there is no `Display` form to switch to. Both are internal
//! diagnostics (a notification-delivery timeout and an HTTP connection-drain
//! deadline) that never surface as an NGSI-LD client's `detail`, so the Debug
//! rendering is tolerated here rather than papered over with an `#[allow]`.

/// Detail messages that break a wording rule for a sanctioned reason.
pub const DETAIL_MESSAGE_BASELINE: &[&str] = &[
    "crates/andromeda-delivery/src/error.rs\tdebug-interpolation\trequest to {endpoint} timed out after {duration:?}",
    "crates/andromeda-http-server/src/error.rs\tdebug-interpolation\tHTTP connections did not drain within {deadline:?}",
];
