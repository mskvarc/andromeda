//! The bound every claimable queue holds a recorded failure to.
//!
//! Each durable queue keeps the latest failure of a row it will retry or has
//! dead-lettered -- `notification_deliveries.last_error`,
//! `pending_side_effects.last_error`, `temporal_append_intents.last_error`,
//! `snapshot_notification_outbox.last_error` and
//! `snapshot_preparation.branch_upgrade_error` -- and every one of those columns
//! carries a check bounding it at [`FAILURE_SUMMARY_MAX_CHARACTERS`]
//! characters. A diagnostic longer than that would fail the very write that
//! records the retry, so it is cut here, once, on the way into the store.

use std::{borrow::Cow, num::NonZeroU32};

/// The most characters a recorded failure summary may carry: the
/// `char_length(last_error) <= 2048` every queue's check states.
pub const FAILURE_SUMMARY_MAX_CHARACTERS: u32 = 2048;

/// [`FAILURE_SUMMARY_MAX_CHARACTERS`] as the non-zero limit
/// [`bounded_failure_summary`] takes.
pub const FAILURE_SUMMARY_LIMIT: NonZeroU32 = NonZeroU32::MIN.saturating_add(FAILURE_SUMMARY_MAX_CHARACTERS - 1);

/// `text` cut to at most `limit` characters, borrowed unchanged when it
/// already fits.
///
/// The count is in characters, as the columns' `char_length` checks count, and
/// the cut falls on a character boundary, so a multi-byte diagnostic is
/// shortened rather than split into invalid UTF-8.
#[must_use]
pub fn bounded_failure_summary(text: &str, limit: NonZeroU32) -> Cow<'_, str> {
    let Ok(limit) = usize::try_from(limit.get()) else {
        return Cow::Borrowed(text);
    };
    match text.char_indices().nth(limit) {
        Some((cut, _)) => Cow::Owned(text[..cut].to_owned()),
        None => Cow::Borrowed(text),
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::failure_summary::{FAILURE_SUMMARY_LIMIT, FAILURE_SUMMARY_MAX_CHARACTERS, bounded_failure_summary};
    use pretty_assertions::assert_eq;
    use std::{borrow::Cow, num::NonZeroU32};

    #[test]
    fn the_limit_is_the_schema_bound() {
        assert_eq!(FAILURE_SUMMARY_LIMIT.get(), FAILURE_SUMMARY_MAX_CHARACTERS);
        assert_eq!(FAILURE_SUMMARY_MAX_CHARACTERS, 2048);
    }

    #[test]
    fn a_summary_within_the_bound_is_borrowed_unchanged() {
        assert_eq!(
            bounded_failure_summary("endpoint refused", FAILURE_SUMMARY_LIMIT),
            Cow::Borrowed("endpoint refused")
        );
    }

    #[test]
    fn a_summary_past_the_bound_is_cut_on_a_character_boundary() {
        let limit = NonZeroU32::new(3).expect("a non-zero limit");
        let cut = bounded_failure_summary("\u{e9}\u{e9}\u{e9}\u{e9}", limit);
        assert_eq!(cut, Cow::<str>::Owned("\u{e9}\u{e9}\u{e9}".to_owned()));
    }

    /// 2048 two-octet characters are 4096 bytes but exactly the bound, so they
    /// are kept whole; a byte count would have cut them in half.
    #[test]
    fn the_bound_counts_characters_not_bytes() {
        let at_bound = "\u{e9}".repeat(2048);
        assert_eq!(bounded_failure_summary(&at_bound, FAILURE_SUMMARY_LIMIT), Cow::Borrowed(at_bound.as_str()));
        let past_bound = format!("{at_bound}x");
        assert_eq!(bounded_failure_summary(&past_bound, FAILURE_SUMMARY_LIMIT).chars().count(), 2048);
    }
}
