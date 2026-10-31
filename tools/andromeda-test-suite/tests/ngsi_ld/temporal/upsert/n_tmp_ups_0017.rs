//! An existing entity gains a geo property.

use andromeda_test_suite::{attribute_matcher::geo_property, broker, evolution_matcher::has_evolution, namespace::Namespace, nth::nth, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        GeoProperty,
        NgsiLdMediaType,
        Property,
        Value,
        evolution::TemporalEntity,
        geometry::{Position, Shape},
    },
};

/// Pins: posting a `GeoProperty` instance for an existing Temporal Evolution answers 204 and the history then holds
/// that `GeoProperty` beside the earlier Property
/// Clause: 5.6.11.4, 5.6.12.4, 4.5.7, 4.7
#[gtest]
#[tokio::test]
async fn n_tmp_ups_0017_an_existing_entity_gains_a_geo_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(120).observed_at("2020-08-01T12:03:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let vehicle_evolution_2 = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_new(-8.5, 41.2)?)).observed_at("2020-08-01T13:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution_2).send().await?.updated()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution, has_evolution("speed", len(eq(1))));
    expect_that!(
        evolution,
        has_evolution(
            "location",
            all!(
                len(eq(1)),
                nth(
                    0,
                    geo_property()
                        .value_member(["type"], "Point")
                        .value_member(["coordinates", "0"], Value::Number("-8.5".parse()?))
                        .observed_at("2020-08-01T13:00:00Z".parse()?)
                )
            )
        )
    );
    Ok(())
}
