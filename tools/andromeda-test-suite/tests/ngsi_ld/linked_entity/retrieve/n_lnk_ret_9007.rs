//! Only the targets that resolve are nested.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    linked_entity::{linked_entities, relationship_objects},
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::InlineJoin},
    model::{Entity, NgsiLdMediaType},
};

/// Pins: Retrieve Entity with join inline of an Entity whose Relationship names a missing and a stored target nests
/// only the stored one, and a Relationship whose only target is missing gains no entity member and keeps naming its
/// target
/// Clause: 4.5.23.1, 4.5.23.2, 4.5.3.2
#[gtest]
#[tokio::test]
async fn n_lnk_ret_9007_only_the_targets_that_resolve_are_nested() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let building_a_id = namespace.urn("Building", "partial-a")?;
    let building_root_id = namespace.urn("Building", "partial-root")?;
    let building_missing_id = namespace.urn("Building", "partial-missing")?;
    let building_a = Entity::builder()
        .id(building_a_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Alice")
        .build()?;
    broker.entities().create(&building_a).send().await?.done()?;
    let building_root = Entity::builder()
        .id(building_root_id.clone())
        .entity_type(building_type.clone())
        .relationship("linkedTo", vec![building_missing_id.clone(), building_a_id.clone()])
        .relationship("hasNeighbours", vec![building_missing_id.clone()])
        .build()?;
    broker.entities().create(&building_root).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_root_id)
        .join(InlineJoin::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    let entity = retrieved.body();
    let nested = linked_entities(entity, &["linkedTo"]);
    expect_that!(nested, len(eq(1)));
    expect_that!(nested, nth(0, has_id(&building_a_id)));
    expect_that!(nested, nth(0, has_attribute("name", property().value("Alice"))));
    expect_that!(linked_entities(entity, &["hasNeighbours"]), is_empty());
    expect_that!(relationship_objects(entity, "hasNeighbours"), elements_are![eq(&building_missing_id)]);
    Ok(())
}
