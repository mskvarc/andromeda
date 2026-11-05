//! The sanctioned baseline of tolerated detail-wording violations.
//!
//! Each entry is one [`crate::detail_message::DetailViolation::key`]: the owning
//! file, the broken rule, and the verbatim message, tab-separated. The gate in
//! [`crate::detail_message`] tolerates exactly these and fails on any new one, so
//! the list can only shrink. A change that fixes a message must delete its entry
//! here, or the gate fails the entry as stale.

/// Detail messages that break a wording rule for a sanctioned reason.
pub const DETAIL_MESSAGE_BASELINE: &[&str] = &[];
