//! The `[notifications]` tables: delivering notifications, evaluating live
//! changes, and draining periodic subscriptions.

pub mod delivery;

use crate::{human_duration::HumanDuration, schema::notifications::delivery::DeliveryConfig};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::num::NonZeroUsize;

/// How live delivery evaluates committed entity changes.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, Copy, PartialEq, Eq)]
#[schemars(deny_unknown_fields)]
#[schemars(description = "How live delivery evaluates committed entity changes against subscriptions.")]
pub struct EvaluationConfig {
    /// The most committed entity changes each tenant buffers for live
    /// evaluation.
    ///
    /// Only `live` delivery reads the buffer; `durable` delivery records every
    /// notification in the commit that raised it. No change is ever dropped:
    /// once the buffer is full, a write waits after its commit until
    /// evaluation makes room, so a burst larger than the buffer slows the
    /// tenant's writes rather than losing notifications. At under half a
    /// kilobyte a slot, a full buffer at the default holds under 4 MB per
    /// tenant.
    #[schemars(
        description = "The most committed entity changes each tenant buffers for live evaluation. No change is dropped: once the buffer is full, a write waits after its commit until evaluation makes room, so a burst larger than the buffer slows the tenant's writes instead of losing notifications. A full buffer holds under 4 MB per tenant at the default. `\"durable\"` delivery does not use the buffer."
    )]
    pub max_queued_changes: NonZeroUsize,
    /// The most committed entity changes evaluated at once.
    ///
    /// Changes of one Entity are evaluated one at a time in commit order;
    /// changes of different Entities run side by side up to this many, each
    /// reading its Entity and recording a notification per matched
    /// subscription, so they share the tenant's pool with request handling.
    #[schemars(
        description = "The most committed entity changes evaluated at once. Changes to one entity are evaluated one at a time in commit order; changes to different entities run side by side, each reading its entity and recording a notification per matched subscription, so they share the tenant's database pool with request handling."
    )]
    pub max_concurrent_changes: NonZeroUsize,
}

impl Default for EvaluationConfig {
    fn default() -> EvaluationConfig {
        EvaluationConfig {
            max_queued_changes: NonZeroUsize::MIN.saturating_add(8191),
            max_concurrent_changes: NonZeroUsize::MIN.saturating_add(31),
        }
    }
}

/// How the periodic (`timeInterval`) notification drain waits.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, Copy, PartialEq, Eq)]
#[schemars(deny_unknown_fields)]
#[schemars(description = "How the drain of periodic (`timeInterval`) notifications waits.")]
pub struct PeriodicConfig {
    /// The longest the drain sleeps while no subscription is due, in
    /// human-readable form (`"30s"`).
    ///
    /// The drain sleeps until the next subscription is due (clause 5.8.6,
    /// floor 1 s); this bounds the sleep when nothing is due, so a change to
    /// the periodic subscriptions is noticed within it.
    #[schemars(
        description = "The longest the periodic notification drain sleeps while no subscription is due. The drain otherwise sleeps until the next subscription is due, so a change to the periodic subscriptions is noticed within this time; due notifications are sent on time either way."
    )]
    pub max_poll_interval: HumanDuration,
}

impl Default for PeriodicConfig {
    fn default() -> PeriodicConfig {
        PeriodicConfig {
            max_poll_interval: HumanDuration::from_secs(30),
        }
    }
}

/// How notifications are delivered, evaluated and scheduled.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, Default, PartialEq, Eq)]
#[schemars(deny_unknown_fields)]
#[schemars(description = "How notifications are delivered, evaluated and scheduled.")]
pub struct NotificationsConfig {
    /// How notifications reach their subscribers.
    #[schemars(description = "How notifications reach their subscribers, the queue that carries them and how long finished deliveries are kept.")]
    pub delivery: DeliveryConfig,
    /// How live delivery evaluates committed entity changes.
    #[schemars(description = "How live delivery evaluates committed entity changes.")]
    pub evaluation: EvaluationConfig,
    /// How the periodic notification drain waits.
    #[schemars(description = "How the periodic (`timeInterval`) notification drain waits.")]
    pub periodic: PeriodicConfig,
}

#[cfg(test)]
mod tests {
    use crate::schema::notifications::NotificationsConfig;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_default_evaluation_buffers_8192_changes_and_evaluates_32_at_once() {
        let config = NotificationsConfig::default();
        assert_eq!(config.evaluation.max_queued_changes.get(), 8192);
        assert_eq!(config.evaluation.max_concurrent_changes.get(), 32);
    }
}
