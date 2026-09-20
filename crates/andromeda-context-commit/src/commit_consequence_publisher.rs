//! Commit-time notification that deferred consequences are available.
//!
//! A commit that writes notification events or side-effect jobs enqueues durable
//! work its delivery drains will eventually claim on their fallback cadence. This
//! edge notification lets the committer wake those drains immediately instead, so
//! the fallback can be widened to keep an idle tenant quiet without adding
//! delivery latency to an active one. It is a latency hint only: a missed signal
//! costs one fallback interval, never a lost delivery.

/// Non-durable edge notification used only to reduce consequence-drain latency.
pub trait CommitConsequencePublisher: Send + Sync {
    /// Signal that at least one notification event was committed.
    fn notifications_available(&self);
    /// Signal that at least one side-effect job was committed.
    fn side_effects_available(&self);
}

/// Publisher for deployments that wake no consequence drains.
#[derive(Debug, Default)]
pub struct NoCommitConsequencePublisher;

impl CommitConsequencePublisher for NoCommitConsequencePublisher {
    fn notifications_available(&self) {}
    fn side_effects_available(&self) {}
}
