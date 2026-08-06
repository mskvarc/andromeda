//! The `attribute_current` projection and the worker that maintains it.
//!
//! `attribute_current` is a denormalised "latest value per TEA" view kept in
//! sync with `attribute_history`. Maintaining it inside the same transaction as
//! the history insert cost 177 ms mean on the live broker, dominated by
//! index-write amplification across ten indexes totalling ~3.5 GB. It is
//! therefore decoupled from the synchronous write path: the committer queues
//! batches on commit and returns, and the worker drains and applies them on its
//! own short-lived connection.
//!
//! Crash semantics: `attribute_history` is the canonical durable source.
//! In-flight events on the channel are lost on process exit, but the startup
//! reconcile refills `attribute_current` from `attribute_history` for every TEA
//! whose latest history row is newer than the current row, so the projection
//! re-converges on every restart.
//!
//! The projection is a storage-side concern, not a policy one: it exists so
//! `q` predicates can be pushed down, and nothing above the store may depend on
//! how -- or whether -- it is maintained.

/// Applying a drained batch, isolating the rows that cannot be applied.
pub mod apply;
/// Collecting queued events into one apply pass.
pub mod drain;
/// The background worker that owns the drain loop.
pub mod materializer;
/// The queue between committers and the materializer, and its two handles.
pub mod queue;
/// Refilling the projection from `attribute_history` on startup.
pub mod reconcile;
