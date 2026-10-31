//! Last n on an explicit observed at time property answers as on the default.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, evolution_matcher::has_evolution, namespace::Namespace, nth::nth, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query::TemporalProperty,
};

/// Pins: Retrieve Temporal Evolution of speed holding 10, 20 and 30 observed hourly and 1000 and 2000 with no
/// `observedAt` answers `lastN` 2 with timeproperty `observedAt` named explicitly with 20 and 30, as without it
/// Clause: 5.7.3.4, 4.11, Table 6.19.3.1-1
#[gtest]
#[tokio::test]
async fn n_tmp_ret_9053_last_n_on_an_explicit_observed_at_time_property_answers_as_on_the_default() -> Result<()> {
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
        .property("speed", Property::builder(30).observed_at("2024-01-01T02:00:00Z".parse()?))
        .property("speed", 2000)
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .time_property(TemporalProperty::ObservedAt)
        .last_n(2)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution("speed", all!(len(eq(2)), nth(0, any_attribute().value(20)), nth(1, any_attribute().value(30))))
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .last_n(2)
        .send()
        .await?;
    let evolution_2 = retrieved_2.body();
    expect_that!(
        evolution_2,
        has_evolution("speed", all!(len(eq(2)), nth(0, any_attribute().value(20)), nth(1, any_attribute().value(30))))
    );
    Ok(())
}
