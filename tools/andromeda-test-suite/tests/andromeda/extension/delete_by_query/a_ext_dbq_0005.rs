//! A time at with nine fraction digits is bad request data and removes nothing.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::{has_evolution, lacks_evolution},
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
};

/// Pins: a temporal delete-by-query whose `timeAt` has nine fractional-second digits answers 400 `BadRequestData`
/// and removes no instance
/// Clause: 4.6.3, 4.11, 6.18.3, 5.5.4
/// Recorded: Andromeda extension, not v1.9.1: clause 6.18.3 binds no DELETE to the temporal entities collection;
/// the `temporal_delete_by_query` extension adds it
#[gtest]
#[tokio::test]
async fn a_ext_dbq_0005_a_time_at_with_nine_fraction_digits_is_bad_request_data_and_removes_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(1).observed_at("2026-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    // No operation of the specification deletes temporal evolutions by a query, so the request is sent as written.
    let request = request_to(
        &broker,
        Method::DELETE,
        &format!("temporal/entities?id=urn%3Angsi-ld%3AVehicle%3A{namespace}-1&drop=speed&timerel=after&timeAt=2026-03-04T05%3A06%3A07.123456789Z"),
    )?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "speed",
            all!(len(eq(1)), nth(0, any_attribute().value(1).observed_at("2026-01-01T00:00:00Z".parse()?)))
        )
    );
    expect_that!(evolution, lacks_evolution("heading"));
    Ok(())
}
