//! A split answer is the answer over the Entity held whole.

use andromeda_test_suite::split_answer::expect_whole_answer;
use googletest::prelude::*;

/// Pins: Query Temporal Evolution of Entities between 12:00 and 13:00 with `splitEntities` true answers, over a Vehicle
/// one broker holds whole and no registration covers, the instances it answers over the same Vehicle split between
/// that broker and a context source: for q speed>90 with and without `lastN` 2, for a local part holding only
/// fuelLevel, for a near geoquery, and for q speed>90|fuelLevel<35
/// Clause: 5.7.4.4
#[gtest]
#[tokio::test]
async fn n_fed_tmp_7463_a_split_answer_is_the_answer_over_the_entity_held_whole() -> Result<()> {
    let cases = [
        super::n_fed_tmp_7458::case()?,
        super::n_fed_tmp_7459::case()?,
        super::n_fed_tmp_7460::case()?,
        super::n_fed_tmp_7461::case()?,
        super::n_fed_tmp_7462::case()?,
    ];
    for case in &cases {
        expect_whole_answer(case).await.and_log_failure();
    }
    Ok(())
}
