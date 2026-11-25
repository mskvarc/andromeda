//! A split lastN counts only the instances that meet q.

use andromeda_test_suite::split_answer::{CaseFilters, Observed::Number, SplitCase, at_minute, expect_split_answer};
use googletest::prelude::*;

/// Local speed 120 at 12:01 and 100 at 12:02, source speed 95 at 12:03 and 80 at 12:04, asked `q=speed>90` with
/// `lastN` 2: the last two instances above 90 are 100 and 95.
pub(super) fn case() -> Result<SplitCase> {
    Ok(SplitCase {
        local: vec![at_minute("speed", Number(120), 1)?, at_minute("speed", Number(100), 2)?],
        source: vec![at_minute("speed", Number(95), 3)?, at_minute("speed", Number(80), 4)?],
        filters: CaseFilters {
            q: Some("speed>90".parse()?),
            last_n: Some(2),
            ..CaseFilters::default()
        },
        answers: vec![at_minute("speed", Number(100), 2)?, at_minute("speed", Number(95), 3)?],
    })
}

/// Pins: Query Temporal Evolution of Entities between 12:00 and 13:00 with `splitEntities` true, q speed>90 and
/// `lastN` 2, over a Vehicle whose local speed is 120 at 12:01 and 100 at 12:02 and whose source speed is 95 at 12:03
/// and 80 at 12:04, answers speed 100 and 95, and the source is asked for the window with no `lastN` and no q
/// Clause: 5.7.4.4
#[gtest]
#[tokio::test]
async fn n_fed_tmp_7458_a_split_last_n_counts_only_the_instances_that_meet_q() -> Result<()> {
    expect_split_answer(&case()?).await
}
