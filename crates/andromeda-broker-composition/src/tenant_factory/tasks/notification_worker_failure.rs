//! Why a resident notification worker stopped.

use crate::tenant_factory::tasks::notification_schedule::classify_notification_failure;
use andromeda_notification::error::NotificationError;
use andromeda_task_runtime::failure::TaskFailure;
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

#[cfg(test)]
mod tests {
    use crate::tenant_factory::tasks::notification_worker_failure::{ChangeChannel, channel_closed};
    use andromeda_task_runtime::failure::FailureClass;

    #[test]
    fn a_closed_change_channel_is_a_transient_failure_naming_the_channel() {
        let failure = channel_closed(ChangeChannel::CsourceSubscription);
        assert_eq!(failure.class(), FailureClass::Transient);
        assert_eq!(failure.to_string(), "csource-subscription change channel closed");
    }
}
