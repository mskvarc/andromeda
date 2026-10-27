//! The `[notifications]` tables: delivering notifications, evaluating live
//! changes, and draining periodic subscriptions.

pub mod delivery;

use crate::{human_duration::HumanDuration, schema::notifications::delivery::DeliveryConfig};
use serde::{Deserialize, Serialize};
use std::num::NonZeroUsize;
use toml_comment::TomlComment;

/// How live delivery evaluates committed entity changes.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EvaluationConfig {
    /// The most committed entity changes each tenant buffers for live
    /// evaluation.
    ///
    /// Only `live` delivery reads the buffer; `durable` delivery records every
    /// notification in the commit that raised it. Every slot is allocated
    /// when the tenant opens: at under half a kilobyte a slot the default
    /// costs under 4 MB per tenant. A burst larger than the buffer, arriving
    /// faster than it is read, loses the notifications of its oldest changes;
    /// each lost change is logged at warn and counted on
    /// `andromeda_notification_live_changes_lost_total`.
    #[toml_comment(inline)]
    pub max_queued_changes: NonZeroUsize,
    /// The most committed entity changes evaluated at once.
    ///
    /// Changes of one Entity are evaluated one at a time in commit order;
    /// changes of different Entities run side by side up to this many, each
    /// reading its Entity and recording a notification per matched
    /// subscription, so they share the tenant's pool with request handling.
    #[toml_comment(inline)]
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
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PeriodicConfig {
    /// The longest the drain sleeps while no subscription is due, in
    /// human-readable form (`"30s"`).
    ///
    /// The drain sleeps until the next subscription is due (clause 5.8.6,
    /// floor 1 s); this bounds the sleep when nothing is due, so a change to
    /// the periodic subscriptions is noticed within it.
    #[toml_comment(inline)]
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
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NotificationsConfig {
    /// How notifications reach their subscribers.
    pub delivery: DeliveryConfig,
    /// How live delivery evaluates committed entity changes.
    pub evaluation: EvaluationConfig,
    /// How the periodic notification drain waits.
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
