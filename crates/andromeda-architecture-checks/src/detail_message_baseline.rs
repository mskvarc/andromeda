//! The sanctioned baseline of tolerated detail-wording violations.
//!
//! Each entry is one [`crate::detail_message::DetailViolation::key`]: the owning
//! file, the broken rule, and the verbatim message, tab-separated. The gate in
//! [`crate::detail_message`] tolerates exactly these and fails on any new one, so
//! the list can only shrink. A change that fixes a message must delete its entry
//! here, or the gate fails the entry as stale.
//!
//! The entry that remains interpolates a [`std::time::Duration`], which
//! implements `Debug` but not `Display`. It is an internal diagnostic (an HTTP
//! connection-drain deadline) that never surfaces as an NGSI-LD client's
//! `detail`, so the Debug rendering is tolerated here rather than papered over
//! with an `#[allow]`.

/// Detail messages that break a wording rule for a sanctioned reason.
pub const DETAIL_MESSAGE_BASELINE: &[&str] =
    &["crates/andromeda-http-server/src/error.rs\tdebug-interpolation\tHTTP connections did not drain within {deadline:?}"];
