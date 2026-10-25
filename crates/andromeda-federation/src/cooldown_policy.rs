//! The broker's own cooldown after a failed forwarded request.
//!
//! Table 5.2.34-1 makes a registration's `management.cooldown` a minimum and
//! lets the broker override it, and clause 6.3.17 lets the broker decline
//! further requests to a failed endpoint; this policy decides how long it
//! declines when the registration sets no cooldown, or a shorter one.

use crate::config::{FederationConfig, FederationConfigError};
use andromeda_retry::{cursor::BackoffCursor, schedule::BackoffSchedule};
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

impl TryFrom<&FederationConfig> for CooldownPolicy {
    type Error = FederationConfigError;

    fn try_from(config: &FederationConfig) -> Result<CooldownPolicy, FederationConfigError> {
        Ok(config.cooldown_schedule()?.map_or(CooldownPolicy::Off, CooldownPolicy::Windowed))
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
    use crate::cooldown_policy::{CooldownEpisode, CooldownPolicy};
    use andromeda_retry::{growth::Growth, jitter::Jitter, schedule::BackoffSchedule};
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
}
