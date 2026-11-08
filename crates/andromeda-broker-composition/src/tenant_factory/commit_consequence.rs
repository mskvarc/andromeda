//! The composition-root publisher that wakes a tenant's consequence drains.

use andromeda_context_commit::commit_consequence_publisher::CommitConsequencePublisher;
use andromeda_task_runtime::wake_trigger::{WakeHandle, WakeRequest};

/// Wakes one tenant's notification-delivery and side-effect drains when a commit
/// enqueues their work.
///
/// The two drains are registered with a widened idle backoff so an inactive
/// tenant stops polling; this publisher restores prompt delivery on an active
/// tenant by waking the matching drain the moment a commit writes its work. A
/// wake that cannot be delivered because a drain has stopped is logged and
/// dropped: the drain's fallback still picks the work up, so a lost edge costs
/// latency, never a delivery.
pub(super) struct RuntimeCommitConsequencePublisher {
    /// Wakes the notification-delivery drain, absent under live delivery where
    /// the commit queues no notification deliveries.
    notifications: Option<WakeHandle>,
    /// Wakes the entity side-effect drain.
    side_effects: WakeHandle,
}

impl RuntimeCommitConsequencePublisher {
    /// Build the publisher from the two drains' wake handles.
    pub(super) const fn new(notifications: Option<WakeHandle>, side_effects: WakeHandle) -> RuntimeCommitConsequencePublisher {
        RuntimeCommitConsequencePublisher { notifications, side_effects }
    }
}

impl CommitConsequencePublisher for RuntimeCommitConsequencePublisher {
    fn notifications_available(&self) {
        if let Some(wake) = &self.notifications {
            report(wake.wake(), "notification-delivery");
        }
    }

    fn side_effects_available(&self) {
        report(self.side_effects.wake(), "entity-side-effect");
    }
}

/// Log a wake that reached a stopped drain; a queued or coalesced wake is silent.
fn report(request: WakeRequest, drain: &str) {
    match request {
        WakeRequest::Scheduled | WakeRequest::Coalesced => (),
        WakeRequest::Closed => {
            tracing::warn!(
                event = "commit_consequence_wake_closed",
                drain,
                "the consequence drain no longer receives wakes"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::tenant_factory::commit_consequence::RuntimeCommitConsequencePublisher;
    use andromeda_context_commit::commit_consequence_publisher::CommitConsequencePublisher;
    use andromeda_task_runtime::wake_trigger::{WakeRequest, wake_channel};

    #[test]
    fn a_commit_with_notification_events_wakes_the_delivery_drain() {
        let (notifications, _notification_receiver) = wake_channel();
        let (side_effects, _side_effect_receiver) = wake_channel();
        let publisher = RuntimeCommitConsequencePublisher::new(Some(notifications.clone()), side_effects.clone());

        publisher.notifications_available();

        // A second wake on the same one-slot channel coalesces, which shows the
        // publisher already queued one; the side-effect channel is still empty.
        assert_eq!(notifications.wake(), WakeRequest::Coalesced, "the notification-delivery drain was woken");
        assert_eq!(side_effects.wake(), WakeRequest::Scheduled, "the side-effect drain was left untouched");
    }

    #[test]
    fn a_commit_with_side_effects_wakes_the_side_effect_drain() {
        let (notifications, _notification_receiver) = wake_channel();
        let (side_effects, _side_effect_receiver) = wake_channel();
        let publisher = RuntimeCommitConsequencePublisher::new(Some(notifications.clone()), side_effects.clone());

        publisher.side_effects_available();

        assert_eq!(side_effects.wake(), WakeRequest::Coalesced, "the side-effect drain was woken");
        assert_eq!(
            notifications.wake(),
            WakeRequest::Scheduled,
            "the notification-delivery drain was left untouched"
        );
    }

    #[test]
    fn a_publisher_without_a_notification_handle_wakes_nothing() {
        let (side_effects, _side_effect_receiver) = wake_channel();
        let publisher = RuntimeCommitConsequencePublisher::new(None, side_effects.clone());

        // Under live delivery a commit queues no notification deliveries, so
        // the publisher holds no handle to the notification-delivery drain, which
        // still runs on its fallback timer for deliveries the tenant database
        // already holds. Announcing notifications is then a silent
        // no-op, and it wakes no other drain in its place.
        publisher.notifications_available();

        assert_eq!(side_effects.wake(), WakeRequest::Scheduled, "the side-effect drain was left untouched");
    }
}
