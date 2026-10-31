//! A write to an unknown entity is resource not found and records no history.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{EntityFragment, ErrorType, NgsiLdMediaType},
};

/// Pins: Append Attributes, Update Attributes and Merge Entity on an identifier never created each answer 404
/// `ResourceNotFound`, and Retrieve Temporal Evolution answers 404
/// Clause: 5.6.2.4, 5.6.3.4, 5.6.17.4
#[gtest]
#[tokio::test]
async fn n_att_app_9010_a_write_to_an_unknown_entity_is_resource_not_found_and_records_no_history() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_never_written_id = namespace.urn("Building", "never-written")?;
    let fragment = EntityFragment::builder().property("humidity", 44).build()?;
    let answer = broker
        .with_responses()
        .entities()
        .append_attributes(&building_never_written_id, &fragment)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let fragment_2 = EntityFragment::builder().property("humidity", 44).build()?;
    let answer_2 = broker
        .with_responses()
        .entities()
        .update_attributes(&building_never_written_id, &fragment_2)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let fragment_3 = EntityFragment::builder().property("humidity", 44).build()?;
    let answer_3 = broker.with_responses().entities().merge(&building_never_written_id, &fragment_3).send().await;
    expect_that!(&answer_3, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&building_never_written_id)
        .send()
        .await;
    expect_that!(&answer_4, answered(StatusCode::NOT_FOUND));
    Ok(())
}
