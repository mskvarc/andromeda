//! Outcome accounting for the durable work queues.

use crate::names::{LABEL_OUTCOME, LABEL_QUEUE, QUEUE_OUTCOMES_TOTAL};
use metrics::counter;
use strum::IntoStaticStr;

/// A durable queue the broker drains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum QueueName {
    /// Notification deliveries awaiting transport.
    NotificationDeliveries,
    /// Batch side effects deferred by `Prefer: respond-async`.
    PendingSideEffects,
    /// Temporal history append intents.
    TemporalAppendIntents,
}

/// What happened to one claimed queue item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum QueueOutcome {
    /// The item completed and was acknowledged.
    Completed,
    /// The item failed and was rescheduled for another attempt.
    Retried,
    /// The item failed for the last time and left the live queue.
    Exhausted,
    /// The lease expired or was taken over before the item could be acknowledged.
    LeaseLost,
    /// The item has failed as many times as the operator threshold names and
    /// stays queued; reported so a wedged sink is visible.
    Stuck,
}

/// Count `count` items of `queue` that ended with `outcome`.
pub fn record_queue_outcome(queue: QueueName, outcome: QueueOutcome, count: u64) {
    if count == 0 {
        return;
    }
    let queue: &'static str = queue.into();
    let outcome: &'static str = outcome.into();
    counter!(QUEUE_OUTCOMES_TOTAL, LABEL_QUEUE => queue, LABEL_OUTCOME => outcome).increment(count);
}

#[cfg(test)]
mod tests {
    use crate::{
        error::TelemetryError,
        names::QUEUE_OUTCOMES_TOTAL,
        queue::{QueueName, QueueOutcome, record_queue_outcome},
        recorder::TelemetryRecorder,
    };

    #[test]
    fn outcomes_render_kebab_case_labels_and_skip_zero_counts() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        recorder.record_locally(|| {
            record_queue_outcome(QueueName::NotificationDeliveries, QueueOutcome::Completed, 3);
            record_queue_outcome(QueueName::PendingSideEffects, QueueOutcome::LeaseLost, 0);
        });
        let rendered = recorder.render();
        assert!(
            rendered.contains(&format!("{QUEUE_OUTCOMES_TOTAL}{{queue=\"notification-deliveries\",outcome=\"completed\"}} 3")),
            "{rendered}"
        );
        assert!(!rendered.contains("pending-side-effects"), "{rendered}");
        Ok(())
    }
}
