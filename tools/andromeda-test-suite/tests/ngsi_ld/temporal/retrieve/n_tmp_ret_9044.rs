//! An instance value type and object types are rendered.

use andromeda_test_suite::{
    attribute_matcher::{property, relationship},
    broker,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, OneOrMany, Property, Relationship, VocabTerm, evolution::TemporalEntity},
};

/// Pins: Create Temporal Evolution with two speed instances carrying valueType and two owner instances carrying
/// objectType [A, B] reads back valueType on every speed instance and both objectTypes on every owner instance
/// Clause: 4.5.2, 4.5.3, 5.7.3
#[gtest]
#[tokio::test]
async fn n_tmp_ret_9044_an_instance_value_type_and_object_types_are_rendered() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_companions_id = namespace.urn("Vehicle", "companions")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let speed_type = namespace.suffixed("Speed").parse::<VocabTerm>()?;
    let person_1_id = namespace.urn("Person", 1)?;
    let person_2_id = namespace.urn("Person", 2)?;
    let object_types = vec![VocabTerm::from(&namespace.entity_type("A")?), VocabTerm::from(&namespace.entity_type("B")?)];
    let vehicle_companions_evolution = TemporalEntity::builder()
        .id(vehicle_companions_id.clone())
        .entity_type(vehicle_type)
        .property(
            "speed",
            Property::builder(20)
                .value_type(speed_type.clone())
                .observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(21)
                .value_type(speed_type.clone())
                .observed_at("2024-01-02T00:00:00Z".parse()?),
        )
        .relationship(
            "owner",
            Relationship::builder(person_1_id.clone())
                .object_type(object_types.clone())
                .observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .relationship(
            "owner",
            Relationship::builder(person_2_id.clone())
                .object_type(object_types.clone())
                .observed_at("2024-01-02T00:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&vehicle_companions_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_companions_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "speed",
            all!(
                len(eq(2)),
                nth(0, property().value(20).value_type(speed_type.clone())),
                nth(1, property().value(21).value_type(speed_type))
            )
        )
    );
    expect_that!(
        evolution,
        has_evolution(
            "owner",
            all!(
                len(eq(2)),
                nth(0, relationship().object(person_1_id).object_type(OneOrMany::try_from(object_types.clone())?)),
                nth(1, relationship().object(person_2_id).object_type(OneOrMany::try_from(object_types)?))
            )
        )
    );
    Ok(())
}
