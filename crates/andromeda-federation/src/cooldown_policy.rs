//! The broker's own cooldown after a failed forwarded request.
//!
//! Table 5.2.34-1 makes a registration's `management.cooldown` a minimum and
//! lets the broker override it, and clause 6.3.17 lets the broker decline
//! further requests to a failed endpoint; this policy decides how long it
//! declines when the registration sets no cooldown, or a shorter one.

use andromeda_configuration::schema::federation::cooldown::{CooldownConfig, CooldownMode};
use andromeda_retry::{cursor::BackoffCursor, growth::Growth, jitter::Jitter, schedule::BackoffSchedule, schedule_error::BackoffScheduleError};
use std::time::Duration;

/// How long the broker holds a failed source back.
#[derive(Debug)]
pub(crate) enum CooldownPolicy {
    /// No broker window: only the registration's own cooldown applies.
    Off,
    /// The windows one episode of failures walks.
    Windowed(BackoffSchedule),
}

impl CooldownPolicy {
    /// The episode a first failure opens, or `None` when the broker applies
    /// no window of its own.
    pub(crate) fn first_episode(&self) -> Option<CooldownEpisode> {
        match self {
            CooldownPolicy::Off => None,
            CooldownPolicy::Windowed(schedule) => Some(CooldownEpisode::new(*schedule)),
        }
    }
}

/// The windows the broker holds a failed source back for: none when the
/// mode is `disabled`, `retry_interval` after every failure when `constant`,
/// and when `exponential` a window doubling from `retry_interval` with each
/// failure of an unbroken episode, jittered upwards by up to itself and never
/// past `max_retry_interval`.
///
/// # Errors
///
/// Returns the [`BackoffScheduleError`] of a mode other than `disabled` with a
/// zero window, or of an `exponential` cap below its first window; the
/// configuration loader asks for the schedule, so a broker never starts
/// with one.
pub const fn cooldown_schedule(config: &CooldownConfig) -> Result<Option<BackoffSchedule>, BackoffScheduleError> {
    let window = config.retry_interval.duration();
    let schedule = match config.mode {
        CooldownMode::Disabled => return Ok(None),
        CooldownMode::Constant => BackoffSchedule::validated(window, window, Growth::Doubling),
        CooldownMode::Exponential => match BackoffSchedule::validated(window, config.max_retry_interval.duration(), Growth::Doubling) {
            Ok(schedule) => Ok(schedule.with_jitter(Jitter::Additive)),
            Err(error) => Err(error),
        },
    };
    match schedule {
        Ok(schedule) => Ok(Some(schedule)),
        Err(error) => Err(error),
    }
}

impl TryFrom<&CooldownConfig> for CooldownPolicy {
    type Error = BackoffScheduleError;

    fn try_from(config: &CooldownConfig) -> Result<CooldownPolicy, BackoffScheduleError> {
        Ok(cooldown_schedule(config)?.map_or(CooldownPolicy::Off, CooldownPolicy::Windowed))
    }
}

/// The windows of one unbroken run of failures, which a success ends.
#[derive(Debug)]
pub(crate) struct CooldownEpisode {
    /// The episode's position in its schedule.
    cursor: BackoffCursor,
    /// The window the latest failure opened.
    window: Duration,
}

impl CooldownEpisode {
    /// The episode a first failure opens on `schedule`.
    pub(crate) fn new(schedule: BackoffSchedule) -> CooldownEpisode {
        let mut cursor = BackoffCursor::new(schedule);
        let window = cursor.advance();
        CooldownEpisode { cursor, window }
    }

    /// The window the latest failure of the episode opened.
    pub(crate) const fn window(&self) -> Duration {
        self.window
    }

    /// Move to the window a further failure of the episode opens.
    pub(crate) fn advance(&mut self) {
        self.window = self.cursor.advance();
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::cooldown_policy::{CooldownEpisode, CooldownPolicy, cooldown_schedule};
    use andromeda_configuration::{
        broker_config::BrokerConfig,
        human_duration::HumanDuration,
        schema::federation::cooldown::{CooldownConfig, CooldownMode},
        validate::validate,
    };
    use andromeda_retry::{growth::Growth, jitter::Jitter, schedule::BackoffSchedule, schedule_error::BackoffScheduleError};
    use std::time::Duration;

    fn windows(mut episode: CooldownEpisode, count: usize) -> Vec<Duration> {
        (0..count)
            .map(|_| {
                let window = episode.window();
                episode.advance();
                window
            })
            .collect()
    }

    #[test]
    fn off_arms_no_window() {
        assert!(CooldownPolicy::Off.first_episode().is_none());
    }

    #[test]
    fn constant_arms_the_same_window_every_episode() {
        let policy = CooldownPolicy::Windowed(BackoffSchedule::constant(Duration::from_secs(5)));
        let five = Duration::from_secs(5);
        pretty_assertions::assert_eq!(windows(policy.first_episode().expect("an episode"), 3), [five, five, five]);
        pretty_assertions::assert_eq!(policy.first_episode().expect("a later episode").window(), five);
    }

    #[test]
    fn exponential_doubles_from_its_first_window() {
        let policy = CooldownPolicy::Windowed(BackoffSchedule::new(Duration::from_secs(1), Duration::from_secs(8), Growth::Doubling));
        let seconds: Vec<u64> = windows(policy.first_episode().expect("an episode"), 5).iter().map(Duration::as_secs).collect();
        pretty_assertions::assert_eq!(seconds, [1, 2, 4, 8, 8]);
    }

    #[test]
    fn jitter_never_lifts_a_window_past_the_cap() {
        let max = Duration::from_secs(4);
        let policy = CooldownPolicy::Windowed(BackoffSchedule::new(Duration::from_secs(1), max, Growth::Doubling).with_jitter(Jitter::Additive));
        assert!(windows(policy.first_episode().expect("an episode"), 1000).iter().all(|window| *window <= max));
    }

    #[test]
    fn a_zero_cooldown_window_is_rejected_unless_the_mode_is_disabled() {
        let zero = |mode| CooldownConfig {
            mode,
            retry_interval: HumanDuration::from_millis(0),
            ..CooldownConfig::default()
        };
        pretty_assertions::assert_eq!(cooldown_schedule(&zero(CooldownMode::Constant)).map(drop), Err(BackoffScheduleError::ZeroFirst));
        pretty_assertions::assert_eq!(
            cooldown_schedule(&zero(CooldownMode::Exponential)).map(drop),
            Err(BackoffScheduleError::ZeroFirst)
        );
        assert!(cooldown_schedule(&zero(CooldownMode::Disabled)).expect("disabled holds no window").is_none());
    }

    #[test]
    fn an_exponential_cap_below_its_first_window_is_rejected() {
        let config = CooldownConfig {
            mode: CooldownMode::Exponential,
            retry_interval: HumanDuration::from_secs(10),
            max_retry_interval: HumanDuration::from_secs(5),
        };
        pretty_assertions::assert_eq!(cooldown_schedule(&config).map(drop), Err(BackoffScheduleError::CeilingBelowFirst));
    }

    /// The configuration validator refuses a cooldown no schedule can be
    /// built from, so the loader never asks this crate; at the edges it
    /// admits, the schedule builds.
    #[test]
    fn every_cooldown_the_configuration_admits_at_its_edges_forms_a_schedule() {
        let edges = [
            (CooldownMode::Disabled, 0, 0),
            (CooldownMode::Constant, 1, 0),
            (CooldownMode::Exponential, 1, 1),
        ];
        for (mode, retry, max) in edges {
            let mut config = BrokerConfig::default();
            config.federation.cooldown = CooldownConfig {
                mode,
                retry_interval: HumanDuration::from_millis(retry),
                max_retry_interval: HumanDuration::from_millis(max),
            };
            assert!(validate(&config).is_ok(), "{mode}");
            assert!(cooldown_schedule(&config.federation.cooldown).is_ok(), "{mode}");
        }
    }

    #[test]
    fn the_default_configuration_arms_no_window() {
        let policy = CooldownPolicy::try_from(&CooldownConfig::default()).expect("the default validates");
        assert!(matches!(policy, CooldownPolicy::Off));
        assert!(policy.first_episode().is_none());
    }
}
