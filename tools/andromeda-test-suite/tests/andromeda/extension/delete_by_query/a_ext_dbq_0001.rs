//! A between window removes its instances and leaves the excluded upper bound.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    problem::answered,
    temporal_delete_by_query::delete_request,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: a temporal delete-by-query of one attribute over the window between t1 and t3 answers 204 and removes the
/// instances at t1 and t2, leaving the instance at t3 that the half-open upper bound excludes
/// Clause: 4.11, 6.18.3
/// Recorded: Andromeda extension, not v1.9.1: clause 6.18.3 binds no DELETE to the temporal entities collection;
/// the `temporal_delete_by_query` extension adds it, here with at most 2 matched instances
#[gtest]
#[tokio::test]
async fn a_ext_dbq_0001_a_between_window_removes_its_instances_and_leaves_the_excluded_upper_bound() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(10).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(20).observed_at("2024-03-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(30).observed_at("2024-06-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    // No operation of the specification deletes temporal evolutions by a query, so the request is sent as written.
    let request = delete_request(
        &broker,
        &format!("id=urn%3Angsi-ld%3AVehicle%3A{namespace}-1&drop=speed&timerel=between&timeAt=2024-01-01T00%3A00%3A00Z&endTimeAt=2024-06-01T00%3A00%3A00Z"),
    )?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .temporal_q(temporal_query::after("1970-01-01T00:00:00Z".parse()?))
        .attrs(vec!["speed".parse()?])
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "speed",
            all!(len(eq(1)), nth(0, any_attribute().value(30).observed_at("2024-06-01T00:00:00Z".parse()?)))
        )
    );
    Ok(())
}
