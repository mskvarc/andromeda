//! The broker's own cooldown after a failed forwarded request.
//!
//! Table 5.2.34-1 makes a registration's `management.cooldown` a minimum and
//! lets the broker override it, and clause 6.3.17 lets the broker decline
//! further requests to a failed endpoint; this policy decides how long it
//! declines when the registration sets no cooldown, or a shorter one.

use crate::config::{CooldownMode, FederationConfig, FederationConfigError};
use backon::{BackoffBuilder, ExponentialBackoff, ExponentialBuilder};
use std::time::Duration;

/// The factor each failure of an unbroken episode multiplies an exponential
/// window by.
const COOLDOWN_FACTOR: f32 = 2.0;

/// How long the broker holds a failed source back.
#[derive(Debug)]
pub(crate) enum CooldownPolicy {
    /// No broker window: only the registration's own cooldown applies.
    Off,
    /// The same window for every failure.
    Constant(Duration),
    /// A window that grows with each failure of an unbroken episode.
    Exponential {
        /// The sequence of windows one episode walks.
        sequence: ExponentialBuilder,
        /// The cap no window exceeds, jitter included.
        max: Duration,
    },
}

impl CooldownPolicy {
    /// The episode a first failure opens, or `None` when the broker applies
    /// no window of its own.
    pub(crate) fn first_episode(&self) -> Option<CooldownEpisode> {
        match self {
            CooldownPolicy::Off => None,
            CooldownPolicy::Constant(window) => Some(CooldownEpisode::Fixed(*window)),
            CooldownPolicy::Exponential { sequence, max } => {
                let mut backoff = sequence.build();
                let window = next_window(&mut backoff, *max);
                Some(CooldownEpisode::Growing { backoff, max: *max, window })
            }
        }
    }
}

impl TryFrom<&FederationConfig> for CooldownPolicy {
    type Error = FederationConfigError;

    fn try_from(config: &FederationConfig) -> Result<CooldownPolicy, FederationConfigError> {
        config.validate_cooldown()?;
        let window = config.cooldown_window.duration();
        Ok(match config.cooldown_mode {
            CooldownMode::Off => CooldownPolicy::Off,
            CooldownMode::Constant => CooldownPolicy::Constant(window),
            CooldownMode::Exponential => {
                let max = config.cooldown_max.duration();
                CooldownPolicy::Exponential {
                    sequence: ExponentialBuilder::default()
                        .with_min_delay(window)
                        .with_max_delay(max)
                        .with_factor(COOLDOWN_FACTOR)
                        .with_jitter()
                        .without_max_times(),
                    max,
                }
            }
        })
    }
}

/// The windows of one unbroken run of failures, which a success ends.
#[derive(Debug)]
pub(crate) enum CooldownEpisode {
    /// Every failure of the episode opens the same window.
    Fixed(Duration),
    /// Every failure of the episode opens the next window of a sequence.
    Growing {
        /// The rest of the episode's sequence.
        backoff: ExponentialBackoff,
        /// The cap no window exceeds.
        max: Duration,
        /// The window the latest failure opened.
        window: Duration,
    },
}

impl CooldownEpisode {
    /// The window the latest failure of the episode opened.
    pub(crate) const fn window(&self) -> Duration {
        match self {
            CooldownEpisode::Fixed(window) | CooldownEpisode::Growing { window, .. } => *window,
        }
    }

    /// Move to the window a further failure of the episode opens.
    pub(crate) fn advance(&mut self) {
        match self {
            CooldownEpisode::Fixed(_) => {}
            CooldownEpisode::Growing { backoff, max, window } => *window = next_window(backoff, *max),
        }
    }
}

/// The next window of `backoff`, clamped to `max`: jitter adds up to the
/// window itself on top of the sequence's own cap. A sequence that ends holds
/// the cap.
fn next_window(backoff: &mut ExponentialBackoff, max: Duration) -> Duration {
    backoff.next().map_or(max, |window| window.min(max))
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::cooldown_policy::{CooldownEpisode, CooldownPolicy};
    use backon::ExponentialBuilder;
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
        let policy = CooldownPolicy::Constant(Duration::from_secs(5));
        let five = Duration::from_secs(5);
        pretty_assertions::assert_eq!(windows(policy.first_episode().expect("an episode"), 3), [five, five, five]);
        pretty_assertions::assert_eq!(policy.first_episode().expect("a later episode").window(), five);
    }

    #[test]
    fn exponential_doubles_from_its_first_window() {
        let max = Duration::from_secs(8);
        let policy = CooldownPolicy::Exponential {
            sequence: ExponentialBuilder::default()
                .with_min_delay(Duration::from_secs(1))
                .with_max_delay(max)
                .with_factor(2.0)
                .without_max_times(),
            max,
        };
        let seconds: Vec<u64> = windows(policy.first_episode().expect("an episode"), 5).iter().map(Duration::as_secs).collect();
        pretty_assertions::assert_eq!(seconds, [1, 2, 4, 8, 8]);
    }

    #[test]
    fn jitter_never_lifts_a_window_past_the_cap() {
        let max = Duration::from_secs(4);
        let policy = CooldownPolicy::Exponential {
            sequence: ExponentialBuilder::default()
                .with_min_delay(Duration::from_secs(1))
                .with_max_delay(max)
                .with_factor(2.0)
                .with_jitter()
                .without_max_times(),
            max,
        };
        assert!(windows(policy.first_episode().expect("an episode"), 1000).iter().all(|window| *window <= max));
    }
}
