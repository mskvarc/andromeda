//! Composition of the durable and live notification delivery configurations.

use andromeda_configuration_scalars::human_duration::HumanDuration;
use andromeda_notification::delivery_config::{DeliveryConfig, DeliveryConfigError};
use serde::{Deserialize, Serialize};
use std::num::NonZeroUsize;
use thiserror::Error;
use toml_comment::TomlComment;

/// Default number of committed entity changes live evaluation works on at once.
const DEFAULT_LIVE_EVALUATION_LANES: NonZeroUsize = NonZeroUsize::MIN.saturating_add(31);

/// Invalid notification configuration.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum NotificationsConfigError {
    /// The durable delivery configuration was rejected by its owner.
    #[error(transparent)]
    Delivery(#[from] DeliveryConfigError),
    /// The interval-notification idle sleep ceiling was configured as zero.
    #[error("notifications.interval_max_sleep must be greater than zero")]
    ZeroIntervalMaxSleep,
    /// The live change buffer was configured to hold no change.
    #[error("notifications.live_change_capacity must be at least 1")]
    ZeroLiveChangeCapacity,
}

/// Notification delivery configuration: the durable delivery queue, live
/// evaluation, and the interval drain, all under one `notifications` section.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone)]
pub struct NotificationsConfig {
    /// Durable notification event and delivery configuration.
    pub delivery: DeliveryConfig,
    /// Ceiling the periodic (`timeInterval`) notification drain's idle backoff
    /// reaches while no subscription change wakes it.
    ///
    /// The drain sleeps until the next subscription is actually due (clause
    /// 5.8.6, floor 1 s); this only bounds how long it waits when nothing is due,
    /// so a change to the interval subscription set is still noticed within it.
    /// Human-readable form (`"30s"`).
    #[toml_comment(inline)]
    pub interval_max_sleep: HumanDuration,
    /// Committed entity changes each tenant buffers for live notification
    /// evaluation.
    ///
    /// Only `live` delivery reads the buffer; `durable` delivery records every
    /// notification in the commit that raised it and allocates a one-change
    /// buffer. Every slot is allocated when the tenant runtime starts, for
    /// every cached runtime: at under half a kilobyte a slot the default costs
    /// under 4 MB per runtime. Live delivery is not durable: a burst larger
    /// than the buffer, arriving faster than the evaluator reads it, loses the
    /// notifications of its oldest changes, and each lost change is logged at
    /// warn and counted on `andromeda_notification_live_changes_lost_total`.
    pub live_change_capacity: usize,
    /// Committed entity changes live notification evaluation works on at once.
    ///
    /// Changes of one Entity are evaluated one at a time in the order they were
    /// committed; changes of different Entities run side by side up to this
    /// many. Each change in flight reads its Entity and claims and records a
    /// notification slot per matched subscription, so the lanes share the
    /// tenant's database pool with request handling. Only `live` delivery
    /// evaluates changes this way.
    #[toml_comment(inline)]
    pub live_evaluation_lanes: NonZeroUsize,
}

impl Default for NotificationsConfig {
    fn default() -> NotificationsConfig {
        NotificationsConfig {
            delivery: DeliveryConfig::default(),
            interval_max_sleep: HumanDuration::from_secs(30),
            live_change_capacity: 8192,
            live_evaluation_lanes: DEFAULT_LIVE_EVALUATION_LANES,
        }
    }
}

impl NotificationsConfig {
    /// Validate the delivery sub-configuration, the interval ceiling and the live buffer.
    ///
    /// # Errors
    ///
    /// Returns [`NotificationsConfigError`] wrapping the delivery error its owner
    /// reported when the delivery configuration is invalid, or
    /// [`NotificationsConfigError::ZeroIntervalMaxSleep`] when the interval idle
    /// ceiling is zero, or [`NotificationsConfigError::ZeroLiveChangeCapacity`]
    /// when the live change buffer holds no change.
    pub fn validate(&self) -> Result<(), NotificationsConfigError> {
        self.delivery.validate()?;
        if self.interval_max_sleep.is_zero() {
            return Err(NotificationsConfigError::ZeroIntervalMaxSleep);
        }
        if self.live_change_capacity == 0 {
            return Err(NotificationsConfigError::ZeroLiveChangeCapacity);
        }
        Ok(())
    }
}
