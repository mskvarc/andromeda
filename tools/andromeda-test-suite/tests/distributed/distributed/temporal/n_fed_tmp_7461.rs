//! A split geoquery keeps the geo property instances that meet it.

use andromeda_test_suite::split_answer::{CaseFilters, Observed::Point, SplitCase, at_minute, expect_split_answer};
use googletest::prelude::*;
use ngsi_ld::{
    model::geometry::{Position, Shape},
    query::geoquery,
};

/// Local `location` about 500 m north of (8.0, 44.0) at 12:01 and source `location` about 900 km north at 12:02,
/// asked `near;maxDistance==2000` around (8.0, 44.0): only the near instance meets it.
pub(super) fn case() -> Result<SplitCase> {
    Ok(SplitCase {
        local: vec![at_minute(
            "location",
            Point {
                longitude: 8.0,
                latitude: 44.0045,
            },
            1,
        )?],
        source: vec![at_minute(
            "location",
            Point {
                longitude: 8.0,
                latitude: 52.1,
            },
            2,
        )?],
        filters: CaseFilters {
            geo_q: Some(geoquery::near_max(2000, Shape::Point(Position::try_new(8.0, 44.0)?)).build()?),
            ..CaseFilters::default()
        },
        answers: vec![at_minute(
            "location",
            Point {
                longitude: 8.0,
                latitude: 44.0045,
            },
            1,
        )?],
    })
}

/// Pins: Query Temporal Evolution of Entities between 12:00 and 13:00 with `splitEntities` true and georel
/// near;maxDistance==2000, over a Vehicle whose local location 500 m from the point was observed at 12:01 and whose
/// source location 900 km away was observed at 12:02, answers only the near location instance
/// Clause: 5.7.4.4, 4.10
#[gtest]
#[tokio::test]
async fn n_fed_tmp_7461_a_split_geoquery_keeps_the_geo_property_instances_that_meet_it() -> Result<()> {
    expect_split_answer(&case()?).await
}
