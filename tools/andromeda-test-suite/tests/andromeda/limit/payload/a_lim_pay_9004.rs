//! Every temporal write of a geometry over the budget is 413.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    config::{Budget, SuiteConfig},
    evolution_matcher::{has_evolution, instance_id_at},
    generated_payload::ring_polygon,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    problem::{problem, problem_status, request_entity_too_large},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Attribute, AttributeName, ErrorType, GeoProperty, NgsiLdMediaType, evolution::TemporalEntity},
};

/// Pins: A geometry one position over the budget answers 413 `RequestEntityTooLarge` naming the budget and the
/// attribute whether it creates a Temporal Evolution, adds an instance to one or modifies one of its instances, and
/// nothing is written
/// Clause: 6.3.2, 5.6.11, 5.6.12, 5.6.14
/// Recorded: ETSI GS CIM 009 sets no size for an entity or a geometry; Andromeda budgets the JSON values of a
/// request (4,000,000), of an entity outside its geometries and JSON literals (1,000,000) and the positions of one
/// geometry (100,000), the stack running the defaults
#[gtest]
#[tokio::test]
async fn a_lim_pay_9004_every_temporal_write_of_a_geometry_over_the_budget_is_413() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let budget = SuiteConfig::get()?.budget(Budget::Position)?;
    let probe_type = namespace.entity_type("Probe")?;
    let location = "location".parse::<AttributeName>()?;
    let refused_id = namespace.urn("Probe", "temporal-refused")?;
    let refused_evolution = TemporalEntity::builder()
        .id(refused_id.clone())
        .entity_type(probe_type.clone())
        .geo_property(
            "location",
            GeoProperty::builder(ring_polygon(budget + 1)?).observed_at("2026-01-01T00:00:00Z".parse()?),
        )
        .build()?;
    let refused = broker.with_responses().temporal().create_or_update(&refused_evolution).send().await;
    expect_that!(
        &refused,
        problem(StatusCode::PAYLOAD_TOO_LARGE, request_entity_too_large())
            .with_detail_containing(&budget.to_string())
            .with_detail_containing("location")
    );
    expect_that!(problem_status(&refused), some(eq(413)));
    let absent = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&refused_id)
        .send()
        .await;
    expect_that!(&absent, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let temporal_id = namespace.urn("Probe", "temporal")?;
    let small_polygon = ring_polygon(10)?;
    let small = TemporalEntity::builder()
        .id(temporal_id.clone())
        .entity_type(probe_type.clone())
        .geo_property(
            "location",
            GeoProperty::builder(small_polygon.clone()).observed_at("2026-01-01T00:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&small).send().await?.created()?;
    let over_budget = TemporalEntity::builder()
        .id(temporal_id.clone())
        .entity_type(probe_type.clone())
        .geo_property(
            "location",
            GeoProperty::builder(ring_polygon(budget + 1)?).observed_at("2026-01-02T00:00:00Z".parse()?),
        )
        .build()?;
    let appended = broker
        .with_responses()
        .temporal()
        .add_attributes(&temporal_id, &over_budget.attributes)
        .send()
        .await;
    expect_that!(
        &appended,
        problem(StatusCode::PAYLOAD_TOO_LARGE, request_entity_too_large())
            .with_detail_containing(&budget.to_string())
            .with_detail_containing("location")
    );
    expect_that!(problem_status(&appended), some(eq(413)));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&temporal_id)
        .send()
        .await?;
    expect_that!(retrieved.body(), has_evolution("location", len(eq(1))));
    let instance = instance_id_at(retrieved.body(), &"location".parse()?, 0)?;
    let replacement = Attribute::from(GeoProperty::builder(ring_polygon(budget + 1)?).build()?);
    let modified = broker
        .with_responses()
        .temporal()
        .modify_instance(&temporal_id, &location, &instance, &replacement)
        .send()
        .await;
    expect_that!(
        &modified,
        problem(StatusCode::PAYLOAD_TOO_LARGE, request_entity_too_large())
            .with_detail_containing(&budget.to_string())
            .with_detail_containing("location")
    );
    expect_that!(problem_status(&modified), some(eq(413)));
    let after = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&temporal_id)
        .send()
        .await?;
    expect_that!(
        after.body(),
        has_evolution("location", all!(len(eq(1)), nth(0, any_attribute().geometry(small_polygon))))
    );
    Ok(())
}
