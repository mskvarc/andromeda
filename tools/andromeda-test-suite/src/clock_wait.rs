//! Waiting for the runner's clock to pass a Unix second. The runner and the broker share the host clock, so the wait is
//! one on the broker's clock too.

use chrono::{DateTime, Utc};
use std::time::Duration;
use tokio::time::sleep;

/// Returns once the runner's clock is past the Unix second `second`, at once when it already is.
pub async fn wait_past(second: i64) {
    sleep(remaining_past(second)).await;
}

/// The time left until the runner's clock is past the Unix second `second`: zero when it already is.
#[must_use]
pub fn remaining_past(second: i64) -> Duration {
    DateTime::from_timestamp(second.saturating_add(1), 0)
        .and_then(|target| (target - Utc::now()).to_std().ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use crate::clock_wait::{remaining_past, wait_past};
    use chrono::Utc;
    use googletest::prelude::*;
    use std::time::Duration;

    #[gtest]
    fn a_second_already_past_leaves_nothing_to_wait() {
        expect_that!(remaining_past(Utc::now().timestamp() - 5), eq(Duration::ZERO));
    }

    #[gtest]
    fn a_later_second_leaves_at_most_its_distance_and_a_second() {
        let remaining = remaining_past(Utc::now().timestamp() + 2);
        expect_that!(remaining, gt(Duration::from_secs(2)));
        expect_that!(remaining, le(Duration::from_secs(3)));
    }

    #[gtest]
    #[tokio::test]
    async fn waiting_past_the_current_second_ends_in_the_next_one() {
        let second = Utc::now().timestamp();
        wait_past(second).await;
        expect_that!(Utc::now().timestamp(), gt(second));
    }
}
