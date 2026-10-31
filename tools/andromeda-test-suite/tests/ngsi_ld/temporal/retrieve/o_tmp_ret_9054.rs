//! A read without last n or a window answers only the observed instances.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::has_evolution,
    header_matcher::absent,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    values_matcher::{has_values, pair},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::{Accept, ContentRange},
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Retrieve Temporal Evolution of speed holding 10, 20 and 30 observed hourly and 1000 and 2000 with no
/// `observedAt`, read with no temporal query and no `lastN`, answers 200 with 10, 20 and 30 in `observedAt` order
/// and no other instance
/// Clause: 4.5.7, 4.5.9, 5.7.3.4, 6.3.10
/// Recorded: Andromeda reading of v1.9.1: clause 5.7.3.4 narrows S1 only by a temporal query, while clause 4.5.7
/// places every instance of the representation "at a particular point in time, which is recorded as a Temporal
/// Property of the instance", clause 6.3.10 pages it by that instant and clause 4.5.9 pairs each value with it; an
/// instance with no value on the requested Temporal Property is therefore left out of every read on it, and stays
/// readable on `createdAt` and `modifiedAt`
#[gtest]
#[tokio::test]
async fn o_tmp_ret_9054_a_read_without_last_n_or_a_window_answers_only_the_observed_instances() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 1000)
        .property("speed", Property::builder(10).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(20).observed_at("2024-01-01T01:00:00Z".parse()?))
        .property("speed", 2000)
        .property("speed", Property::builder(30).observed_at("2024-01-01T02:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let headed = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    expect_that!(headed.head.typed::<ContentRange>(), absent());
    let retrieved = &headed.outcome;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "speed",
            all!(
                len(eq(3)),
                nth(0, any_attribute().value(10).observed_at("2024-01-01T00:00:00Z".parse()?)),
                nth(1, any_attribute().value(20).observed_at("2024-01-01T01:00:00Z".parse()?)),
                nth(2, any_attribute().value(30).observed_at("2024-01-01T02:00:00Z".parse()?))
            )
        )
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_values(&vehicle_id)
        .send()
        .await?;
    let values = retrieved_2.body();
    expect_that!(values, has_values("speed", len(eq(3))));
    expect_that!(values, has_values("speed", nth(0, pair().value(10).time("2024-01-01T00:00:00Z".parse()?))));
    expect_that!(values, has_values("speed", nth(2, pair().value(30).time("2024-01-01T02:00:00Z".parse()?))));
    Ok(())
}
