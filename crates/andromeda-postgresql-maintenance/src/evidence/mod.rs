//! The evidence the controller records in the platform database, and the
//! migration-owned codes each piece is stored as.
//!
//! Every translation between a controller value and its stored code is written
//! as exhaustive matches in both directions, so a new outcome, decision or
//! target fails to compile here, which is where its persisted number is decided.

/// How one maintenance action ended.
pub(crate) mod action_result;
/// What one tenant's inspection came to.
pub(crate) mod inspection_result;
/// The decision last recorded for one target.
pub(crate) mod recorded_decision;
/// The number a maintenance target is stored as.
pub(crate) mod target_code;
