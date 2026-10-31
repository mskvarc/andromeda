//! An unknown id beside an existing one answers 207.

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

/// Pins: a Batch Entity Delete naming an existing and an unknown entity answers 207 with the existing id in success
/// and a `ResourceNotFound` error for the unknown one, and the existing entity is gone
/// Clause: 5.6.10.4, 5.6.10.5, 6.17.3.1, 5.6.6.4
#[gtest]
#[tokio::test]
async fn n_bat_del_0002_an_unknown_id_beside_an_existing_one_answers_207() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_unknown_id = namespace.urn("Building", "unknown")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .property("name", "Eiffel Tower")
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let ids = [building_id.clone(), building_unknown_id.clone()];
    let report = broker.entities().batch_delete(&ids).send().await?.partial()?;
    expect_that!(&report.success, len(eq(1)));
    expect_that!(&report.success, nth(0, eq(&building_id)));
    expect_that!(&report.errors, len(eq(1)));
    expect_that!(
        &report.errors,
        nth(0, batch_error().entity(&building_unknown_id).problem(ErrorType::ResourceNotFound))
    );
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
