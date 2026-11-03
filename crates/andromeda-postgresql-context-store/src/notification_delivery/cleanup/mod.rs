//! Reaping delivered rows, dead-lettered rows, and the events they leave behind.
//!
//! Three independent passes cover the ways a `notification_events` row can
//! stop being owed a delivery: [`delivered::purge_delivered_before`] reaps
//! deliveries acknowledged as sent, [`exhausted::purge_exhausted_before`] reaps
//! deliveries the worker gave up on and that were kept only as evidence, and
//! [`orphan_events::purge_orphan_events_before`] reaps events left with zero
//! referencing deliveries by any path the first two do not observe directly: a
//! subscription that vanished between the match and the delivery insert, or a
//! `Subscription` deletion whose `ON DELETE CASCADE` removed every delivery a
//! moment the other two passes were not looking.
//!
//! Each pass sweeps every branch: the live branch and every Snapshot branch keep
//! their own queue. A branch reaped between the enumeration and the sweep shows
//! up as a missing schema and is skipped.
//!
//! No statement removes more than [`branch_sweep::PURGE_BATCH_ROWS`] rows. A
//! pass repeats batches, each in its own transaction, so the rows it holds
//! locked, the keys it holds in memory and the time one statement runs do not
//! depend on how many rows are past the cutoff.

/// Visiting every branch of a pass and the size of one batch.
pub(crate) mod branch_sweep;
/// Reaping deliveries acknowledged as sent.
pub mod delivered;
/// Reaping deliveries the worker dead-lettered.
pub mod exhausted;
/// Reaping events no delivery references.
pub mod orphan_events;
/// One batch of deliveries past a retention clock, and the walk over a branch.
pub(crate) mod retention_batch;
