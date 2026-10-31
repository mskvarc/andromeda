//! The change events a committed durable side effect publishes once the replay
//! that removed its job has landed.
//!
//! A durable side-effect replay commits its temporal writes, its notification
//! events and the removal of its job in one transaction, under the job's lease,
//! but the in-memory change-sink republication is a local fan-out outside that
//! transaction. Holding the change events here until the replay commit has
//! landed means only the worker whose replay removed the job publishes them, so
//! the best-effort live channel carries a single publication per job.

use andromeda_context_commit::{change_event::ChangeEvent, store_instant::StoreInstant};

/// The live-branch change events a committed side effect must publish,
/// deferred until the replay that removed the job has landed.
#[derive(Debug)]
pub(crate) struct CommittedSideEffect {
    /// Change events to publish once the replay has landed.
    pub(crate) change_events: Vec<ChangeEvent>,
    /// The store instant of the commit that made the changes, the job's
    /// enqueueing commit rather than the replay.
    pub(crate) committed_at: StoreInstant,
}
