//! An entity the temporal api created is counted as current state holds it.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeKind, OneOrMany, Property, evolution::TemporalEntity},
};

/// Pins: an entity created through Create Temporal Evolution exists in current state, and discovery counts it
/// exactly as one created through the core API: its type and attribute are available with one entity and one
/// instance
/// Clause: 5.6.11.4, 5.7.7.4, 5.7.10.4, 5.7.1.4
/// Recorded: the spec does not say whether Create Temporal Evolution of an Entity also creates the entity in current
/// state (5.6.11.4 creates "the provided Temporal Evolution of an Entity"); Andromeda creates it, and discovery then
/// reports what current state holds
#[gtest]
#[tokio::test]
async fn o_dsc_tyi_0009_an_entity_the_temporal_api_created_is_counted_as_current_state_holds_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let height_name = namespace.attribute("height")?;
    let evolution = TemporalEntity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property(height_name.clone(), Property::builder(1).observed_at("2026-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&evolution).send().await?.created()?;
    let retrieved = broker.with_accept(Accept::any()).entities().retrieve(&building_id).send().await?;
    let entity = retrieved.body();
    expect_that!(entity.entity_type, eq(&OneOrMany::one(building_type.clone())));
    expect_that!(entity, has_attribute(&namespace.suffixed("height"), any_attribute().value(1)));
    let information = broker.with_accept(Accept::any()).entity_types().retrieve(&building_type).send().await?;
    expect_that!(information.entity_count, eq(1));
    expect_that!(information.attribute_details, len(eq(1)));
    let detail = information.attribute_details.first().or_fail()?;
    expect_that!(detail.attribute_name, eq(&height_name));
    expect_that!(detail.attribute_types.as_deref(), some(elements_are![eq(&AttributeKind::Property)]));
    let attribute = broker.with_accept(Accept::any()).attributes().retrieve(&height_name).send().await?;
    expect_that!(attribute.attribute_count, some(eq(1)));
    expect_that!(attribute.type_names.as_deref(), some(elements_are![eq(&building_type)]));
    Ok(())
}
