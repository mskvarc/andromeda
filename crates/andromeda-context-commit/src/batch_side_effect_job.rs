//! Durable work produced by the opt-in asynchronous batch path.
//!
//! Only the live branch defers side effects: a request scoped to a Snapshot
//! runs them inline, so a job always finishes a live-branch write and names
//! no branch. The write's temporal history is not part of the job: it is
//! committed as queued intents with the write itself.

use crate::change_event::ChangeEvent;

/// The change events deferred by one live-branch entity write.
#[derive(Debug)]
pub struct BatchSideEffectJob {
    /// Changes whose notifications the queuing commit did not record as
    /// durable deliveries, and which whichever broker drains the job therefore
    /// still owes its subscribers.
    ///
    /// A broker that records durable deliveries at commit leaves this empty:
    /// its notifications are already queued, and carrying the changes as well
    /// would notify them a second time when a live-delivery broker drains the
    /// job.
    pub change_events: Vec<ChangeEvent>,
}

impl BatchSideEffectJob {
    /// Whether the job carries no change, so a write need not queue it.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.change_events.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use crate::batch_side_effect_job::BatchSideEffectJob;

    #[test]
    fn a_job_without_changes_is_empty() {
        let job = BatchSideEffectJob { change_events: Vec::new() };
        assert!(job.is_empty());
    }
}
