//! A geometry far over the position budget is 413 and creates nothing.

use andromeda_test_suite::{
    broker,
    budget::Budget,
    config::SuiteConfig,
    generated_payload::ring_polygon,
    namespace::Namespace,
    problem::{problem, problem_status, request_entity_too_large},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: Create Entity whose location Polygon holds three times the geometry budget answers 413
/// `RequestEntityTooLarge` naming the attribute and the budget, like one a position over, and creates nothing
/// Clause: 6.3.2
/// Recorded: ETSI GS CIM 009 sets no size for an entity or a geometry; Andromeda budgets the JSON values of a
/// request (4,000,000), of an entity outside its geometries and JSON literals (1,000,000) and the positions of one
/// geometry (100,000), the stack running the defaults; the geometry is refused as it is read, before its
/// coordinates are kept
#[gtest]
#[tokio::test]
async fn a_lim_pay_9021_a_geometry_far_over_the_position_budget_is_413_and_creates_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let budget = SuiteConfig::get()?.budget(Budget::Position)?;
    let entity_id = namespace.urn("Probe", "far-over")?;
    let entity = Entity::builder()
        .id(entity_id.clone())
        .entity_type(namespace.entity_type("Probe")?)
        .geo_property("location", ring_polygon(budget * 3)?)
        .build()?;
    let refused = broker.with_responses().entities().create(&entity).send().await;
    expect_that!(
        &refused,
        problem(StatusCode::PAYLOAD_TOO_LARGE, request_entity_too_large())
            .with_detail_containing("location")
            .with_detail_containing(&budget.to_string())
    );
    expect_that!(problem_status(&refused), some(eq(413)));
    let absent = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&entity_id)
        .send()
        .await;
    expect_that!(&absent, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
