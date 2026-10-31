//! A geo property instance reads back as a geo property.

use andromeda_test_suite::{attribute_matcher::geo_property, broker, evolution_matcher::has_evolution, namespace::Namespace, nth::nth, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        GeoProperty,
        NgsiLdMediaType,
        Value,
        evolution::TemporalEntity,
        geometry::{Position, Shape},
    },
};

/// Pins: a Temporal Evolution created with a `GeoProperty` instance reads back with that instance as a
/// `GeoProperty` holding its Point and `observedAt`
/// Clause: 5.6.11.4, 4.5.7, 4.7, 5.7.3.4
#[gtest]
#[tokio::test]
async fn n_tmp_ups_0011_a_geo_property_instance_reads_back_as_a_geo_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_new(-8.5, 41.2)?)).observed_at("2020-08-01T12:03:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
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
            "location",
            all!(
                len(eq(1)),
                nth(
                    0,
                    geo_property()
                        .value_member(["type"], "Point")
                        .value_member(["coordinates", "0"], Value::Number("-8.5".parse()?))
                        .value_member(["coordinates", "1"], Value::Number("41.2".parse()?))
                        .observed_at("2020-08-01T12:03:00Z".parse()?)
                )
            )
        )
    );
    Ok(())
}
