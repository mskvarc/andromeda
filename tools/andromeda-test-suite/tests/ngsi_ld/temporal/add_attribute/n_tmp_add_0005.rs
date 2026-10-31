//! A geo property instance reads back as a geo property.

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

/// Pins: Add Attributes with a `GeoProperty` instance answers 204, and the Temporal Evolution then holds it as a
/// `GeoProperty` with its geometry
/// Clause: 5.6.12.4, 6.20.3.1, 4.5.9
#[gtest]
#[tokio::test]
async fn n_tmp_add_0005_a_geo_property_instance_reads_back_as_a_geo_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(120).observed_at("2020-09-01T12:03:00Z".parse()?))
        .property("speed", Property::builder(80).observed_at("2020-09-01T12:05:00Z".parse()?))
        .property("fuelLevel", Property::builder(67).observed_at("2020-09-01T12:03:00Z".parse()?))
        .property("fuelLevel", Property::builder(53).observed_at("2020-09-01T13:05:00Z".parse()?))
        .property(
            "fuelLevel",
            Property::builder(40)
                .dataset_id(namespace.dataset_id("Dataset", "fuel")?)
                .observed_at("2020-09-01T14:07:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let instances = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_new(-8.5, 41.2)?)).observed_at("2020-09-01T14:00:00Z".parse()?),
        )
        .build()?
        .attributes;
    broker.temporal().add_attributes(&vehicle_id, &instances).send().await?;
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
                )
            )
        )
    );
    Ok(())
}
