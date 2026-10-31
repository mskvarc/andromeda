//! Last n takes the most recent observed instances and none without observed at.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::has_evolution,
    header_matcher::absent,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::{Accept, ContentRange},
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Retrieve Temporal Evolution of speed holding 10, 20 and 30 observed hourly from 2024-01-01T00:00:00Z and
/// 1000 and 2000 with no `observedAt` answers `lastN` 2 with 20 and 30, `lastN` 1 with 30 and `lastN` 10 with 200,
/// no Content-Range and the three observed instances only
/// Clause: 5.7.3.4, Table 5.2.21-1, 4.5.7, 6.3.10
#[gtest]
#[tokio::test]
async fn n_tmp_ret_9050_last_n_takes_the_most_recent_observed_instances_and_none_without_observed_at() -> Result<()> {
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
        .last_n(2)
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
                len(eq(2)),
                nth(0, any_attribute().value(20).observed_at("2024-01-01T01:00:00Z".parse()?)),
                nth(1, any_attribute().value(30).observed_at("2024-01-01T02:00:00Z".parse()?))
            )
        )
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .last_n(1)
        .send()
        .await?;
    let evolution_2 = retrieved_2.body();
    expect_that!(
        evolution_2,
        has_evolution(
            "speed",
            all!(len(eq(1)), nth(0, any_attribute().value(30).observed_at("2024-01-01T02:00:00Z".parse()?)))
        )
    );
    let headed_2 = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .last_n(10)
        .send()
        .await?;
    expect_that!(headed_2.head.typed::<ContentRange>(), absent());
    let retrieved_3 = &headed_2.outcome;
    let evolution_3 = retrieved_3.body();
    expect_that!(
        evolution_3,
        has_evolution(
            "speed",
            all!(
                len(eq(3)),
                contains(any_attribute().value(10)),
                contains(any_attribute().value(20)),
                contains(any_attribute().value(30)),
                not(contains(any_attribute().value(1000))),
                not(contains(any_attribute().value(2000)))
            )
        )
    );
    Ok(())
}
