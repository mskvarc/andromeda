//! A repeated id deletes once and reports the repeat.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    partial_outcome::PartialSuccess,
    problem::problem,
    report_matcher::batch_error,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: a Batch Entity Delete naming one existing entity twice answers 207: the first occurrence deletes it and is
/// in success, the second is a `ResourceNotFound` error for the same id
/// Clause: 5.5.11.4, 5.6.10.4, 5.6.10.5, 6.17.3.1
#[gtest]
#[tokio::test]
async fn n_bat_del_0003_a_repeated_id_deletes_once_and_reports_the_repeat() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .property("name", "Eiffel Tower")
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let ids = [building_id.clone(), building_id.clone()];
    let report = broker.entities().batch_delete(&ids).send().await?.partial()?;
    expect_that!(&report.success, len(eq(1)));
    expect_that!(&report.success, nth(0, eq(&building_id)));
    expect_that!(&report.errors, len(eq(1)));
    expect_that!(&report.errors, nth(0, batch_error().entity(&building_id).problem(ErrorType::ResourceNotFound)));
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
