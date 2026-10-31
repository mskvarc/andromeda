//! An observation of 61 levels is refused and not recorded.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{answered, problem},
    value::object,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType, Property, Value, evolution::TemporalEntity},
};

/// Pins: Create Temporal Evolution of an observation whose value nests 61 objects answers 400 `BadRequestData`
/// naming p and the 32-level limit, and neither the Temporal Evolution nor the entity exists
/// Clause: 5.6.11.4, 5.5.4
#[gtest]
#[tokio::test]
async fn a_lim_dep_9002_an_observation_of_61_levels_is_refused_and_not_recorded() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let nesting_probe_id = namespace.urn("NestingProbe", 1)?;
    let nesting_probe_type = namespace.entity_type("NestingProbe")?;
    let nesting_probe_evolution = TemporalEntity::builder()
        .id(nesting_probe_id.clone())
        .entity_type(nesting_probe_type)
        .property(
            "p",
            Property::builder((0..61).fold(Value::from("leaf"), |inner, _| object([("k", inner)]))).observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .build()?;
    let answer = broker.with_responses().temporal().create_or_update(&nesting_probe_evolution).send().await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_detail_containing("'https://uri.etsi.org/ngsi-ld/default-context/p'")
            .with_detail_containing("32 levels")
    );
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&nesting_probe_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let answer_3 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&nesting_probe_id)
        .send()
        .await;
    expect_that!(&answer_3, answered(StatusCode::NOT_FOUND));
    Ok(())
}
