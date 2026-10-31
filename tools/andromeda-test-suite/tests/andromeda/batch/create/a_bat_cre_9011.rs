//! A batch whose every entity holds nul reports each and creates nothing.

use andromeda_test_suite::{broker, namespace::Namespace, partial_outcome::PartialSuccess, problem::answered, report_matcher::batch_error};
use googletest::prelude::*;
use http::StatusCode;
use indexmap::IndexMap;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType, Value},
};

/// Pins: Batch Entity Creation whose every Entity holds U+0000 in a Property value answers 207 with no success and a `BadRequestData` error for each Entity, and creates none of them
/// Clause: 5.6.7.4, 5.5.4
/// Recorded: Andromeda limit, not v1.9.1: JSON admits U+0000 in a string and the specification sets no rule against it, but Andromeda cannot store the character, so it refuses each entity carrying it as input it cannot take
#[gtest]
#[tokio::test]
async fn a_bat_cre_9011_a_batch_whose_every_entity_holds_nul_reports_each_and_creates_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_1_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let entities = [
        Entity::builder()
            .id(vehicle_1_id.clone())
            .entity_type(vehicle_type.clone())
            .property("brandName", "nul\u{0}here")
            .build()?,
        Entity::builder()
            .id(vehicle_2_id.clone())
            .entity_type(vehicle_type.clone())
            .property("brandName", Value::Object(IndexMap::from([("model".to_owned(), Value::from("nul\u{0}here"))])))
            .build()?,
    ];
    let report = broker.entities().batch_create(&entities).send().await?.partial()?;
    expect_that!(&report.success, is_empty());
    expect_that!(&report.errors, len(eq(2)));
    expect_that!(&report.errors, contains(batch_error().entity(&vehicle_1_id).problem(ErrorType::BadRequestData)));
    expect_that!(&report.errors, contains(batch_error().entity(&vehicle_2_id).problem(ErrorType::BadRequestData)));
    for id in [&vehicle_1_id, &vehicle_2_id] {
        let answer = broker
            .with_responses()
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(id)
            .send()
            .await;
        expect_that!(&answer, answered(StatusCode::NOT_FOUND));
    }
    Ok(())
}
