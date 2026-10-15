//! Why a resident notification worker stopped.

use crate::tenant_factory::tasks::notification_schedule::classify_notification_failure;
use andromeda_notification::error::NotificationError;
use andromeda_task_runtime::{failure::TaskFailure, keyed_lanes::LaneStopped};
use strum::Display;
use thiserror::Error;

/// A change channel a resident notification worker listens on.
#[derive(Clone, Copy, Debug, Display, Eq, PartialEq)]
#[strum(serialize_all = "kebab-case")]
pub(super) enum ChangeChannel {
    /// Changes to the tenant's entity subscriptions.
    Subscription,
    /// Changes to the tenant's entities.
    Entity,
    /// Changes to the tenant's context-source subscriptions.
    CsourceSubscription,
    /// Changes to the tenant's context-source registrations.
    Registration,
}

/// The failure a resident notification worker stops with.
#[derive(Debug, Error)]
pub(super) enum NotificationWorkerFailure {
    /// Notification processing failed.
    #[error(transparent)]
    Notification(NotificationError),
    /// A change channel the worker listens on closed.
    #[error("{0} change channel closed")]
    ChannelClosed(ChangeChannel),
    /// A lane evaluating live entity changes stopped running.
    #[error("live notification evaluation: {0}")]
    EvaluationLaneStopped(LaneStopped),
}

/// Classify a notification processing failure inside a resident worker.
pub(super) fn worker_notification_failure(error: NotificationError) -> TaskFailure<NotificationWorkerFailure> {
    classify_notification_failure(error).map_cause(NotificationWorkerFailure::Notification)
}

/// The failure a worker stops with when one of its change channels closes.
///
/// A closed channel means the tenant's change producers were torn down; a
/// restarted worker subscribes afresh, so the failure is transient.
pub(super) const fn channel_closed(channel: ChangeChannel) -> TaskFailure<NotificationWorkerFailure> {
    TaskFailure::transient(NotificationWorkerFailure::ChannelClosed(channel))
}

/// The failure the entity notification worker stops with when one of its live
/// evaluation lanes stopped.
///
/// The lanes no longer keep the order of an Entity's changes; a restarted
/// worker starts them afresh, so the failure is transient.
pub(super) const fn evaluation_lane_stopped(stopped: LaneStopped) -> TaskFailure<NotificationWorkerFailure> {
    TaskFailure::transient(NotificationWorkerFailure::EvaluationLaneStopped(stopped))
}

#[cfg(test)]
mod tests {
    use crate::tenant_factory::tasks::notification_worker_failure::{ChangeChannel, channel_closed, evaluation_lane_stopped};
    use andromeda_task_runtime::{failure::FailureClass, keyed_lanes::LaneStopped};

    #[test]
    fn a_closed_change_channel_is_a_transient_failure_naming_the_channel() {
        let failure = channel_closed(ChangeChannel::CsourceSubscription);
        assert_eq!(failure.class(), FailureClass::Transient);
        assert_eq!(failure.to_string(), "csource-subscription change channel closed");
    }

    #[test]
    fn a_stopped_evaluation_lane_is_a_transient_failure() {
        let failure = evaluation_lane_stopped(LaneStopped);
        assert_eq!(failure.class(), FailureClass::Transient);
        assert_eq!(failure.to_string(), "live notification evaluation: the lane of a dispatched item has stopped");
    }
}
