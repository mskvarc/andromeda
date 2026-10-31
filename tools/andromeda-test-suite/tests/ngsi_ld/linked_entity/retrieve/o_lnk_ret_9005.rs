//! Every target of a multi target relationship is descended through.

use andromeda_test_suite::{broker, identity_matcher::has_id, linked_entity::linked_entities, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::InlineJoin},
    model::{Entity, NgsiLdMediaType},
};

/// Pins: Retrieve Entity with join inline over a Relationship naming two Entities that both link one inner Entity
/// nests no second hop at `joinLevel` 1 and nests the inner Entity under both at `joinLevel` 2, and join flat at
/// `joinLevel` 2 answers all four Entities
/// Clause: 4.5.23.1, 4.5.23.2, 4.5.23.3
/// Recorded: clause 5.7.1.4 nests a target unless its URI has been previously encountered, without saying whether a
/// sibling branch counts; Andromeda counts only the path from the root, so a target two branches reach is nested
/// under both
#[gtest]
#[tokio::test]
async fn o_lnk_ret_9005_every_target_of_a_multi_target_relationship_is_descended_through() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let building_inner_id = namespace.urn("Building", "depth-inner")?;
    let building_first_id = namespace.urn("Building", "depth-first")?;
    let building_second_id = namespace.urn("Building", "depth-second")?;
    let building_root_id = namespace.urn("Building", "depth-root")?;
    let building_inner = Entity::builder()
        .id(building_inner_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Inner")
        .build()?;
    broker.entities().create(&building_inner).send().await?.done()?;
    let building_first = Entity::builder()
        .id(building_first_id.clone())
        .entity_type(building_type.clone())
        .relationship("linkedTo", building_inner_id.clone())
        .build()?;
    broker.entities().create(&building_first).send().await?.done()?;
    let building_second = Entity::builder()
        .id(building_second_id.clone())
        .entity_type(building_type.clone())
        .relationship("linkedTo", building_inner_id.clone())
        .build()?;
    broker.entities().create(&building_second).send().await?.done()?;
    let building_root = Entity::builder()
        .id(building_root_id.clone())
        .entity_type(building_type.clone())
        .relationship("hasNeighbours", vec![building_first_id.clone(), building_second_id.clone()])
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
    let neighbours = linked_entities(retrieved.body(), &["hasNeighbours"]);
    expect_that!(neighbours, len(eq(2)));
    expect_that!(
        neighbours.iter().all(|neighbour| linked_entities(neighbour, &["linkedTo"]).is_empty()),
        eq(true)
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_root_id)
        .join(InlineJoin::Inline)
        .join_level(2.try_into()?)
        .send()
        .await?;
    let neighbours_2 = linked_entities(retrieved_2.body(), &["hasNeighbours"]);
    expect_that!(neighbours_2, len(eq(2)));
    expect_that!(
        neighbours_2
            .iter()
            .filter(|neighbour| linked_entities(neighbour, &["linkedTo"])
                .first()
                .is_some_and(|inner| inner.id.as_ref() == Some(&building_inner_id)))
            .count(),
        eq(2)
    );
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened(&building_root_id)
        .join_level(2.try_into()?)
        .send()
        .await?;
    let entities_3 = retrieved_3.body().as_slice();
    expect_that!(entities_3, len(eq(4)));
    expect_that!(entities_3, contains(has_id(&building_inner_id)));
    expect_that!(entities_3, contains(has_id(&building_first_id)));
    expect_that!(entities_3, contains(has_id(&building_second_id)));
    expect_that!(entities_3, contains(has_id(&building_root_id)));
    Ok(())
}
