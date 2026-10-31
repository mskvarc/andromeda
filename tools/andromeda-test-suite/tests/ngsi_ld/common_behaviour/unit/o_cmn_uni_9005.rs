//! A list property unit reaches the temporal evolution.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, evolution_matcher::has_evolution, namespace::Namespace, nth::nth, outcome::Done};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ListProperty, NgsiLdMediaType, Value},
};

/// Pins: Retrieve Temporal Evolution of an Entity created with a `ListProperty` observed with `unitCode` MMT
/// answers its one instance with the unit and the list
/// Clause: 4.5.21.2, 4.5.9, 4.3.1
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default
#[gtest]
#[tokio::test]
async fn o_cmn_uni_9005_a_list_property_unit_reaches_the_temporal_evolution() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", "unit-history")?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .list_property(
            "depths",
            ListProperty::builder(vec![Value::from(300), Value::from(120)])
                .unit_code(UnitCode::Mmt)
                .observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&building_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "depths",
            all!(
                len(eq(1)),
                nth(0, any_attribute().unit_code(UnitCode::Mmt).list_length(2).list_value(0, 300).list_value(1, 120))
            )
        )
    );
    Ok(())
}
