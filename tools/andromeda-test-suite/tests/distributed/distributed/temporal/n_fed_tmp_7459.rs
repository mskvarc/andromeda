//! A split q keeps the instances of every part that meet it.

use andromeda_test_suite::split_answer::{CaseFilters, Observed::Number, SplitCase, at_minute, expect_split_answer};
use googletest::prelude::*;

/// Local speed 120 at 12:01 and 100 at 12:02, source speed 95 at 12:03 and 80 at 12:04, asked `q=speed>90` with no
/// `lastN`: every instance above 90, of either part.
pub(super) fn case() -> Result<SplitCase> {
    Ok(SplitCase {
        local: vec![at_minute("speed", Number(120), 1)?, at_minute("speed", Number(100), 2)?],
        source: vec![at_minute("speed", Number(95), 3)?, at_minute("speed", Number(80), 4)?],
        filters: CaseFilters {
            q: Some("speed>90".parse()?),
            ..CaseFilters::default()
        },
        answers: vec![
            at_minute("speed", Number(120), 1)?,
            at_minute("speed", Number(100), 2)?,
            at_minute("speed", Number(95), 3)?,
        ],
    })
}

/// Pins: Query Temporal Evolution of Entities between 12:00 and 13:00 with `splitEntities` true and q speed>90, over
/// a Vehicle whose local speed is 120 at 12:01 and 100 at 12:02 and whose source speed is 95 at 12:03 and 80 at 12:04,
/// answers speed 120, 100 and 95 and not 80
/// Clause: 5.7.4.4
#[gtest]
#[tokio::test]
async fn n_fed_tmp_7459_a_split_q_keeps_the_instances_of_every_part_that_meet_it() -> Result<()> {
    expect_split_answer(&case()?).await
}
