//! An instance without observed at is outside every observed at window.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
    query::temporal_query::{self, TemporalProperty},
};

/// Pins: Speed 55 written without `observedAt`: the Temporal Evolution since 1970 on the default axis is 404
/// `ResourceNotFound`, and since 2026-01-01 on `modifiedAt` it holds the instance 55 with no `observedAt`
/// Clause: 4.11, 5.2.21, 5.7.3.4
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default
#[gtest]
#[tokio::test]
async fn o_tmp_ret_9039_an_instance_without_observed_at_is_outside_every_observed_at_window() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_unobserved_id = namespace.urn("Vehicle", "unobserved")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_unobserved = Entity::builder()
        .id(vehicle_unobserved_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 55)
        .build()?;
    broker.entities().create(&vehicle_unobserved).send().await?.done()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_unobserved_id)
        .temporal_q(temporal_query::after("1970-01-01T00:00:00Z".parse()?))
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_unobserved_id)
        .temporal_q(temporal_query::after("2026-01-01T00:00:00Z".parse()?).time_property(TemporalProperty::ModifiedAt))
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().value(55).without(Member::ObservedAt))))
    );
    Ok(())
}
