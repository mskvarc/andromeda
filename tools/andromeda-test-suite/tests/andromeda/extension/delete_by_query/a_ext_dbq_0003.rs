//! A delete over the bound is too many results and removes nothing.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    problem::problem,
    wire::request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: a temporal delete-by-query matching more instances than the configured bound of 2 answers 403
/// `TooManyResults` and removes nothing
/// Clause: 5.5.2, 6.3.2, 6.18.3
/// Recorded: Andromeda extension, not v1.9.1: clause 6.18.3 binds no DELETE to the temporal entities collection;
/// the `temporal_delete_by_query` extension adds it, here with at most 2 matched instances
#[gtest]
#[tokio::test]
async fn a_ext_dbq_0003_a_delete_over_the_bound_is_too_many_results_and_removes_nothing() -> Result<()> {
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
    let request = request_to(
        &broker,
        Method::DELETE,
        &format!("temporal/entities?id=urn%3Angsi-ld%3AVehicle%3A{namespace}-1&drop=speed"),
    )?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::FORBIDDEN, ErrorType::TooManyResults));
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
            all!(
                len(eq(3)),
                nth(0, any_attribute().observed_at("2024-01-01T00:00:00Z".parse()?)),
                nth(1, any_attribute().observed_at("2024-03-01T00:00:00Z".parse()?)),
                nth(2, any_attribute().observed_at("2024-06-01T00:00:00Z".parse()?))
            )
        )
    );
    Ok(())
}
