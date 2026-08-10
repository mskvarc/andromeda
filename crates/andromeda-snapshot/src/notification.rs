//! Snapshot status notification port.

use andromeda_ngsi_ld_model::snapshot::Snapshot;
use futures::future::BoxFuture;

/// Delivers one snapshot status notification without coupling policy to transport runtime.
pub trait SnapshotNotificationSink: Send + Sync + 'static {
    /// Deliver the notification represented by the current snapshot state.
    fn deliver<'a>(&'a self, snapshot: &'a Snapshot) -> BoxFuture<'a, ()>;
}
