//! A bulk upsert of an unchanged feed adds no instance.

use andromeda_test_suite::{broker, evolution_matcher::has_evolution, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property},
    query::temporal_query::{self, TemporalProperty},
};

/// Pins: a Batch Entity Upsert of three `WeatherObserved` repeated unchanged answers 204 and leaves each with the
/// one temperature instance the first upsert recorded
/// Clause: 4.3.1, 5.8.6, 4.5.7, 5.6.8.4
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating ... whenever the current state is modified"); Andromeda records a Core write only for the
/// attribute instances it modified, so a feed repeating its last report adds nothing
#[gtest]
#[tokio::test]
async fn o_tmp_cwh_9026_a_bulk_upsert_of_an_unchanged_feed_adds_no_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let weather_observed_feed_1_id = namespace.urn("WeatherObserved", "feed-1")?;
    let weather_observed_type = namespace.entity_type("WeatherObserved")?;
    let weather_observed_feed_2_id = namespace.urn("WeatherObserved", "feed-2")?;
    let weather_observed_feed_3_id = namespace.urn("WeatherObserved", "feed-3")?;
    let entities = [
        Entity::builder()
            .id(weather_observed_feed_1_id.clone())
            .entity_type(weather_observed_type.clone())
            .property("temperature", Property::builder(11).observed_at("2026-10-19T10:00:00Z".parse()?))
            .build()?,
        Entity::builder()
            .id(weather_observed_feed_2_id.clone())
            .entity_type(weather_observed_type.clone())
            .property("temperature", Property::builder(12).observed_at("2026-10-19T10:00:00Z".parse()?))
            .build()?,
        Entity::builder()
            .id(weather_observed_feed_3_id.clone())
            .entity_type(weather_observed_type.clone())
            .property("temperature", Property::builder(13).observed_at("2026-10-19T10:00:00Z".parse()?))
            .build()?,
    ];
    broker.entities().batch_upsert(&entities).send().await?.done()?;
    let entities_2 = [
        Entity::builder()
            .id(weather_observed_feed_1_id.clone())
            .entity_type(weather_observed_type.clone())
            .property("temperature", Property::builder(11).observed_at("2026-10-19T10:00:00Z".parse()?))
            .build()?,
        Entity::builder()
            .id(weather_observed_feed_2_id.clone())
            .entity_type(weather_observed_type.clone())
            .property("temperature", Property::builder(12).observed_at("2026-10-19T10:00:00Z".parse()?))
            .build()?,
        Entity::builder()
            .id(weather_observed_feed_3_id.clone())
            .entity_type(weather_observed_type.clone())
            .property("temperature", Property::builder(13).observed_at("2026-10-19T10:00:00Z".parse()?))
            .build()?,
    ];
    broker.entities().batch_upsert(&entities_2).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?).time_property(TemporalProperty::ModifiedAt))
        .entity_type(&weather_observed_type)
        .attrs(vec!["temperature".parse()?])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(3)));
    expect_that!(
        page.items(),
        contains(all!(has_id(&weather_observed_feed_1_id), has_evolution("temperature", len(eq(1)))))
    );
    expect_that!(
        page.items(),
        contains(all!(has_id(&weather_observed_feed_2_id), has_evolution("temperature", len(eq(1)))))
    );
    expect_that!(
        page.items(),
        contains(all!(has_id(&weather_observed_feed_3_id), has_evolution("temperature", len(eq(1)))))
    );
    Ok(())
}
