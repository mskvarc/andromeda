//! A split lastN reaches instances earlier than the source's own last ones.

use andromeda_test_suite::split_answer::{CaseFilters, Observed::Number, SplitCase, at_minute, expect_split_answer};
use googletest::prelude::*;

/// Source speed 120 at 12:01, 100 at 12:02, 70 at 12:05 and 60 at 12:06, local fuelLevel 50 at 12:01, asked
/// `q=speed>90` with `lastN` 2: the Vehicle is selected through its earlier speeds, which are the last two above 90,
/// and fuelLevel, which `q` does not name, keeps its instance.
pub(super) fn case() -> Result<SplitCase> {
    Ok(SplitCase {
        local: vec![at_minute("fuelLevel", Number(50), 1)?],
        source: vec![
            at_minute("speed", Number(120), 1)?,
            at_minute("speed", Number(100), 2)?,
            at_minute("speed", Number(70), 5)?,
            at_minute("speed", Number(60), 6)?,
        ],
        filters: CaseFilters {
            q: Some("speed>90".parse()?),
            last_n: Some(2),
            ..CaseFilters::default()
        },
        answers: vec![
            at_minute("speed", Number(120), 1)?,
            at_minute("speed", Number(100), 2)?,
            at_minute("fuelLevel", Number(50), 1)?,
        ],
    })
}

/// Pins: Query Temporal Evolution of Entities between 12:00 and 13:00 with `splitEntities` true, q speed>90 and
/// `lastN` 2, over a Vehicle whose local part holds only fuelLevel 50 at 12:01 and whose source speed is 120 at
/// 12:01, 100 at 12:02, 70 at 12:05 and 60 at 12:06, selects the Vehicle and answers speed 120 and 100 beside
/// fuelLevel 50
/// Clause: 5.7.4.4
#[gtest]
#[tokio::test]
async fn n_fed_tmp_7460_a_split_last_n_reaches_instances_earlier_than_the_source_last_ones() -> Result<()> {
    expect_split_answer(&case()?).await
}
