//! A geometry over the position budget is 413 in either encoding.

use andromeda_test_suite::{
    broker,
    config::{Budget, SuiteConfig},
    generated_payload::{json_string, polygon, ring_polygon},
    namespace::Namespace,
    problem::{problem, problem_status, request_entity_too_large},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Create Entity whose location Polygon holds one position more than the geometry budget, as a `GeoJSON`
/// object or as a JSON string, answers 413 `RequestEntityTooLarge` naming the attribute and the budget, and creates
/// nothing
/// Clause: 6.3.2, 4.7.2
/// Recorded: ETSI GS CIM 009 sets no size for an entity or a geometry; Andromeda budgets the JSON values of a
/// request (4,000,000), of an entity outside its geometries and JSON literals (1,000,000) and the positions of one
/// geometry (100,000), the stack running the defaults
#[gtest]
#[tokio::test]
async fn a_lim_pay_9003_a_geometry_over_the_position_budget_is_413_in_either_encoding() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let budget = SuiteConfig::get()?.budget(Budget::Position)?;
    let probe_type = namespace.entity_type("Probe")?;
    let object_id = namespace.urn("Probe", "object")?;
    let string_id = namespace.urn("Probe", "string")?;
    let as_object = Entity::builder()
        .id(object_id.clone())
        .entity_type(probe_type.clone())
        .geo_property("location", ring_polygon(budget + 1)?)
        .build()?;
    let refused = broker.with_responses().entities().create(&as_object).send().await;
    expect_that!(
        &refused,
        problem(StatusCode::PAYLOAD_TOO_LARGE, request_entity_too_large())
            .with_detail_containing("location")
            .with_detail_containing(&budget.to_string())
    );
    expect_that!(problem_status(&refused), some(eq(413)));
    // A GeoProperty whose value is a JSON string holding the geometry is a member the model does not write.
    let carrier = Entity::builder().id(string_id.clone()).entity_type(probe_type.clone()).build()?;
    let location: RawJson = format!(r#"{{"type":"GeoProperty","value":{}}}"#, json_string(&polygon(budget + 1)?)).parse()?;
    let document = UncheckedDocument::from_entity(&carrier, Lossless::Normalized).set_member("location", &location);
    let as_string = broker
        .send_unchecked(broker.unchecked(&CreateEntity::builder().entity(&carrier).build())?.document(&document))
        .await;
    expect_that!(
        &as_string,
        problem(StatusCode::PAYLOAD_TOO_LARGE, request_entity_too_large())
            .with_detail_containing("location")
            .with_detail_containing(&budget.to_string())
    );
    expect_that!(problem_status(&as_string), some(eq(413)));
    for id in [&object_id, &string_id] {
        let absent = broker
            .with_responses()
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(id)
            .send()
            .await;
        expect_that!(&absent, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    }
    Ok(())
}
