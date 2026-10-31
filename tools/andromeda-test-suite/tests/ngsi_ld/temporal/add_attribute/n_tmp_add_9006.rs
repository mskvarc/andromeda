//! An instance added through the temporal api keeps its sub attributes.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, property},
    broker,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, Value, evolution::TemporalEntity},
};

/// Pins: Add Attributes to Temporal Evolution with a speed instance carrying accuracy 0.7 and `observedAt` reads
/// back that instance with its accuracy
/// Clause: 5.6.12, 4.5.2
#[gtest]
#[tokio::test]
async fn n_tmp_add_9006_an_instance_added_through_the_temporal_api_keeps_its_sub_attributes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_added_sub_attribute_id = namespace.urn("Vehicle", "added-sub-attribute")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_added_sub_attribute_evolution = TemporalEntity::builder()
        .id(vehicle_added_sub_attribute_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(20).observed_at("2024-01-01T00:00:00Z".parse()?))
        .build()?;
    broker
        .temporal()
        .create_or_update(&vehicle_added_sub_attribute_evolution)
        .send()
        .await?
        .created()?;
    let instances = TemporalEntity::builder()
        .id(vehicle_added_sub_attribute_id.clone())
        .entity_type(vehicle_type.clone())
        .property(
            "speed",
            Property::builder(25)
                .observed_at("2024-01-02T00:00:00Z".parse()?)
                .property("accuracy", Value::Number("0.7".parse()?)),
        )
        .build()?
        .attributes;
    broker.temporal().add_attributes(&vehicle_added_sub_attribute_id, &instances).send().await?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_added_sub_attribute_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "speed",
            all!(
                len(eq(2)),
                nth(0, any_attribute().value(20).without_sub_attribute("accuracy")),
                nth(
                    1,
                    any_attribute()
                        .value(25)
                        .sub_attribute("accuracy", property().value(Value::Number("0.7".parse()?)))
                )
            )
        )
    );
    Ok(())
}
