//! A list property unit renders as a member in every representation.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    body_json::{json_absent, json_equals},
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    versioned_answer::send_versioned,
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{RepresentationSpelling, RetrieveEntity},
    },
    model::{Entity, ListProperty, Lossless, NgsiLdMediaType, Value},
};

/// Pins: Retrieve Entity of a `ListProperty` with `unitCode` MMT answers the unit as a string member beside
/// `valueList` normalized, and beside the values without type in the concise representation
/// Clause: 4.5.21.2, 4.5.2.2
#[gtest]
#[tokio::test]
async fn n_cmn_uni_9004_a_list_property_unit_renders_as_a_member_in_every_representation() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", "unit-representations")?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .list_property(
            "depths",
            ListProperty::builder(vec![Value::from(300), Value::from(120)]).unit_code(UnitCode::Mmt),
        )
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "depths",
            any_attribute().unit_code(UnitCode::Mmt).list_length(2).list_value(0, 300).list_value(1, 120)
        )
    );
    // The concise reading of a list is held as received, since the concise reader keeps no wrapper to tell it from a Property.
    let concise = RetrieveEntity::builder()
        .id(&building_id)
        .format(Lossless::Concise)
        .spelling(RepresentationSpelling::default().in_options())
        .build();
    let answer = send_versioned(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), &concise, None).await?;
    expect_that!(answer.status, eq(StatusCode::OK));
    expect_that!(&answer.body, json_equals("$.depths.unitCode", "MMT"));
    expect_that!(&answer.body, json_absent("$.depths.type"));
    Ok(())
}
