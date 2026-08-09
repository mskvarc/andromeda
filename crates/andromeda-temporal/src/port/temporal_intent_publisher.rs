//! Commit-time notification that deferred temporal work is available.

/// Non-durable edge notification used only to reduce drain latency.
pub trait TemporalIntentPublisher: Send + Sync {
    /// Signal that at least one committed intent is available.
    fn intents_available(&self);
}

/// Publisher used by native-history profiles, where no intent is created.
#[derive(Debug, Default)]
pub struct NoTemporalIntentPublisher;

impl TemporalIntentPublisher for NoTemporalIntentPublisher {
    fn intents_available(&self) {}
}
