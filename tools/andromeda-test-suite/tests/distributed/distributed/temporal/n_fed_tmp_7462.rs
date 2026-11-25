//! A split disjunction keeps an instance another Attribute satisfies.

use andromeda_test_suite::split_answer::{CaseFilters, Observed::Number, SplitCase, at_minute, expect_split_answer};
use googletest::prelude::*;

/// Local speed 95 and fuelLevel 40 at 12:01, source speed 80 at 12:02, asked `q=speed>90|fuelLevel<35`: speed 95 meets
/// it, fuelLevel 40 meets it through speed 95 with `fuelLevel` reduced to it, and speed 80 meets neither term.
pub(super) fn case() -> Result<SplitCase> {
    Ok(SplitCase {
        local: vec![at_minute("speed", Number(95), 1)?, at_minute("fuelLevel", Number(40), 1)?],
        source: vec![at_minute("speed", Number(80), 2)?],
        filters: CaseFilters {
            q: Some("speed>90|fuelLevel<35".parse()?),
            ..CaseFilters::default()
        },
        answers: vec![at_minute("speed", Number(95), 1)?, at_minute("fuelLevel", Number(40), 1)?],
    })
}

/// Pins: Query Temporal Evolution of Entities between 12:00 and 13:00 with `splitEntities` true and
/// q speed>90|fuelLevel<35, over a Vehicle whose local speed 95 and fuelLevel 40 were observed at 12:01 and whose
/// source speed 80 was observed at 12:02, answers speed 95 and fuelLevel 40 and not speed 80
/// Clause: 5.7.4.4, 4.9
#[gtest]
#[tokio::test]
async fn n_fed_tmp_7462_a_split_disjunction_keeps_an_instance_another_attribute_satisfies() -> Result<()> {
    expect_split_answer(&case()?).await
}
