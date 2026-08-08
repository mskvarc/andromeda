//! Consumer-declared port for the read-your-writes barrier.
//!
//! The `attribute_current` projection is eventually consistent with respect
//! to committed writes. Queries that compile a `q` predicate sample the
//! issue watermark and wait for the materialiser to catch up, so a read
//! after a committed write sees that write's attributes in the projection.
//!
//! The concrete barrier lives in the crate that owns the materialiser
//! queue; this trait lets other crates depend on the capability without
//! depending on the concrete implementation or on `tokio::sync`.

use std::fmt::Debug;

/// Port for waiting until the `attribute_current` projection has caught up
/// with committed writes.
///
/// Sampling and waiting is a single call: the implementation snapshots the
/// current issue watermark, then waits until the materialiser's applied
/// watermark reaches it.
#[async_trait::async_trait]
pub trait ProjectionBarrier: Send + Sync + Debug {
    /// Wait until every batch queued before this call has been applied.
    #[must_use = "projection barriers must be awaited"]
    async fn wait_for_pending_writes(&self);
}
