//! A deprecated common code is admitted and read back.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::{Done, Upserted},
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Create Entity and Create Temporal Evolution with `unitCode` A1, a common code Recommendation 20 marks
/// deprecated, answer 201, and both representations read the code back
/// Clause: 4.5.2.2, 5.2.5, 5.6.1, 5.6.11
/// Recorded: reference `[15]` pins UN/CEFACT Recommendation 20 Revision 9, which lists A1; a status a later
/// revision gives a code does not make a value written to Revision 9 non-conforming
#[gtest]
#[tokio::test]
async fn n_cmn_uni_0008_a_deprecated_common_code_is_admitted_and_read_back() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_current_id = namespace.urn("Device", "current")?;
    let device_type = namespace.entity_type("Device")?;
    let device_history_id = namespace.urn("Device", "history")?;
    let device_current = Entity::builder()
        .id(device_current_id.clone())
        .entity_type(device_type.clone())
        .property("heat", Property::builder(5).unit_code(UnitCode::A1))
        .build()?;
    broker.entities().create(&device_current).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_current_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("heat", any_attribute().value(5).unit_code(UnitCode::A1)));
    let device_history_evolution = TemporalEntity::builder()
        .id(device_history_id.clone())
        .entity_type(device_type.clone())
        .property(
            "heat",
            Property::builder(5).unit_code(UnitCode::A1).observed_at("2026-01-01T00:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&device_history_evolution).send().await?.created()?;
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&device_history_id)
        .send()
        .await?;
    let evolution = retrieved_2.body();
    expect_that!(
        evolution,
        has_evolution("heat", all!(len(eq(1)), nth(0, any_attribute().value(5).unit_code(UnitCode::A1))))
    );
    Ok(())
}
