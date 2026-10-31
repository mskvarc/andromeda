//! A replaced instance keeps the sub attributes of its replacement.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::{has_evolution, instance_id_at},
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Attribute, AttributeName, NgsiLdMediaType, Property, Value, evolution::TemporalEntity},
};

/// Pins: Modify Attribute instance of a speed instance carrying accuracy 0.9 and precision 2 with a replacement
/// carrying accuracy 0.5 alone reads back the instance with accuracy 0.5 and no precision
/// Clause: 5.6.14.4, 4.5.2
#[gtest]
#[tokio::test]
async fn n_tmp_mod_9004_a_replaced_instance_keeps_the_sub_attributes_of_its_replacement() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_replaced_sub_attributes_id = namespace.urn("Vehicle", "replaced-sub-attributes")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_replaced_sub_attributes_evolution = TemporalEntity::builder()
        .id(vehicle_replaced_sub_attributes_id.clone())
        .entity_type(vehicle_type)
        .property(
            "speed",
            Property::builder(20)
                .observed_at("2024-01-01T00:00:00Z".parse()?)
                .property("accuracy", Value::Number("0.9".parse()?))
                .property("precision", 2),
        )
        .build()?;
    broker
        .temporal()
        .create_or_update(&vehicle_replaced_sub_attributes_evolution)
        .send()
        .await?
        .created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_replaced_sub_attributes_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    let instance_id = instance_id_at(evolution, &"speed".parse()?, 0)?;
    let instance = Attribute::from(
        Property::builder(20)
            .observed_at("2024-01-01T00:00:00Z".parse()?)
            .property("accuracy", Value::Number("0.5".parse()?))
            .build()?,
    );
    broker
        .temporal()
        .modify_instance(&vehicle_replaced_sub_attributes_id, &"speed".parse::<AttributeName>()?, &instance_id, &instance)
        .send()
        .await?;
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_replaced_sub_attributes_id)
        .send()
        .await?;
    let evolution_2 = retrieved_2.body();
    expect_that!(
        evolution_2,
        has_evolution(
            "speed",
            all!(
                len(eq(1)),
                nth(
                    0,
                    any_attribute()
                        .instance_id(&instance_id)
                        .without_sub_attribute("precision")
                        .sub_attribute("accuracy", any_attribute().value(Value::Number("0.5".parse()?)))
                )
            )
        )
    );
    Ok(())
}
