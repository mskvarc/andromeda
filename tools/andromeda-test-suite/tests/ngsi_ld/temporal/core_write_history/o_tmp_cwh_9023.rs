//! Three upserts restating humidity record one humidity and two temperature instances.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, evolution_matcher::has_evolution, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property},
    query::temporal_query::TemporalProperty,
};

/// Pins: three Batch Entity Upserts of a `WeatherObserved` carrying temperature and humidity with one `observedAt`,
/// the first two identical and the third changing only temperature, leave one humidity instance and two temperature
/// instances in its Temporal Evolution
/// Clause: 4.3.1, 5.8.6, 4.5.7, 5.6.8.4
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating ... whenever the current state is modified"); Andromeda records a Core write only for the
/// attribute instances it modified, a modification being what clause 5.8.6 calls a change
#[gtest]
#[tokio::test]
async fn o_tmp_cwh_9023_three_upserts_restating_humidity_record_one_humidity_and_two_temperature_instances() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let weather_observed_restated_id = namespace.urn("WeatherObserved", "restated")?;
    let weather_observed_type = namespace.entity_type("WeatherObserved")?;
    let entities = [Entity::builder()
        .id(weather_observed_restated_id.clone())
        .entity_type(weather_observed_type.clone())
        .property("temperature", Property::builder(20).observed_at("2026-10-19T10:00:00Z".parse()?))
        .property("humidity", Property::builder(50).observed_at("2026-10-19T10:00:00Z".parse()?))
        .build()?];
    broker.entities().batch_upsert(&entities).send().await?.done()?;
    let entities_2 = [Entity::builder()
        .id(weather_observed_restated_id.clone())
        .entity_type(weather_observed_type.clone())
        .property("temperature", Property::builder(20).observed_at("2026-10-19T10:00:00Z".parse()?))
        .property("humidity", Property::builder(50).observed_at("2026-10-19T10:00:00Z".parse()?))
        .build()?];
    broker.entities().batch_upsert(&entities_2).send().await?.done()?;
    let entities_3 = [Entity::builder()
        .id(weather_observed_restated_id.clone())
        .entity_type(weather_observed_type.clone())
        .property("temperature", Property::builder(21).observed_at("2026-10-19T10:00:00Z".parse()?))
        .property("humidity", Property::builder(50).observed_at("2026-10-19T10:00:00Z".parse()?))
        .build()?];
    broker.entities().batch_upsert(&entities_3).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&weather_observed_restated_id)
        .time_property(TemporalProperty::ModifiedAt)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution, has_evolution("humidity", all!(len(eq(1)), nth(0, any_attribute().value(50)))));
    expect_that!(
        evolution,
        has_evolution(
            "temperature",
            all!(
                len(eq(2)),
                contains(any_attribute().value(20).value(20)),
                contains(any_attribute().value(20)).times(eq(1)),
                contains(any_attribute().value(21).value(21)),
                contains(any_attribute().value(21)).times(eq(1))
            )
        )
    );
    Ok(())
}
