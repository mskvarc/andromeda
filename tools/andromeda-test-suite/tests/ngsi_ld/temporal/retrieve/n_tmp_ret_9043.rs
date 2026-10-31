//! A sub attribute of a sub attribute is rendered.

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

/// Pins: Create Temporal Evolution of speed with two instances whose accuracy carries a method sub-attribute reads back
/// speed.accuracy.method in both instances
/// Clause: 5.7.3, 4.5.2
#[gtest]
#[tokio::test]
async fn n_tmp_ret_9043_a_sub_attribute_of_a_sub_attribute_is_rendered() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_nested_sub_attribute_id = namespace.urn("Vehicle", "nested-sub-attribute")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_nested_sub_attribute_evolution = TemporalEntity::builder()
        .id(vehicle_nested_sub_attribute_id.clone())
        .entity_type(vehicle_type)
        .property(
            "speed",
            Property::builder(20)
                .observed_at("2024-01-01T00:00:00Z".parse()?)
                .property("accuracy", Property::builder(Value::Number("0.9".parse()?)).property("method", "gps")),
        )
        .property(
            "speed",
            Property::builder(21)
                .observed_at("2024-01-02T00:00:00Z".parse()?)
                .property("accuracy", Property::builder(Value::Number("0.8".parse()?)).property("method", "radar")),
        )
        .build()?;
    broker
        .temporal()
        .create_or_update(&vehicle_nested_sub_attribute_evolution)
        .send()
        .await?
        .created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_nested_sub_attribute_id)
        .send()
        .await?;
    expect_that!(
        retrieved.body(),
        has_evolution(
            "speed",
            all!(
                len(eq(2)),
                nth(
                    0,
                    any_attribute()
                        .value(20)
                        .sub_attribute("accuracy", any_attribute().sub_attribute("method", property().value("gps")))
                ),
                nth(
                    1,
                    any_attribute()
                        .value(21)
                        .sub_attribute("accuracy", any_attribute().sub_attribute("method", property().value("radar")))
                )
            )
        )
    );
    Ok(())
}
