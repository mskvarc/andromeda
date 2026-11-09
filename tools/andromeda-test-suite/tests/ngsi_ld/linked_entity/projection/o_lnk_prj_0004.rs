//! The first relationship in the pick list to reach a shared flat target decides its projection.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute, projected_member_count},
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    type_matcher::has_type,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::{Join, projection::Projection},
};

/// Pins: under a flat join a target that two picked Relationships share is appended once, projected by the
/// Relationship that comes first in the pick list: a group there narrows it and a later bare Relationship or
/// another group neither widens nor merges it, while a bare Relationship there leaves it whole, whichever order the
/// entity holds `relA` and `relB` in
/// Clause: 4.21, 4.5.23.3, 5.7.1.5, 5.7.2.5
/// Recorded: 5.7.1.5 and 5.7.2.5 append a target "unless a target URI has been previously encountered" and 4.21
/// addresses a Linked Entity Term to the Linked Entity reached through its Relationship, but no clause says which
/// Relationship's projection applies when two reach the same appended target, so the file is kind O; the pick list
/// is the only order the request defines, an entity's members having none in JSON-LD. Ruled by the user
/// (2026-10-05)
#[gtest]
#[tokio::test]
async fn o_lnk_prj_0004_the_first_relationship_in_the_pick_list_to_reach_a_shared_flat_target_decides_its_projection() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_b_id = namespace.urn("Building", "b")?;
    let building_type = namespace.entity_type("Building")?;
    let building_a_id = namespace.urn("Building", "a")?;
    let entities = [
        Entity::builder()
            .id(building_b_id.clone())
            .entity_type(building_type.clone())
            .property("category", "commercial")
            .property("name", "Depot")
            .build()?,
        Entity::builder()
            .id(building_a_id.clone())
            .entity_type(building_type.clone())
            .relationship("relA", building_b_id.clone())
            .relationship("relB", building_b_id.clone())
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // Query Entities: relA{category} reaches b first, so the bare relB does not widen it
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .ids(vec![building_a_id.clone()])
        .entity_type(&building_type)
        .join(Join::Flat)
        .join_level(1.try_into()?)
        .pick("relA{category},relB,id".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items().get(1).map(projected_member_count), some(eq(1)));
    let item = page.items().get(1).or_fail()?;
    expect_that!(item.id, none());
    expect_that!(page.items(), nth(0, has_id(&building_a_id)));
    expect_that!(
        page.items(),
        nth(1, all!(has_attribute("category", any_attribute().value("commercial")), lacks_attribute("name")))
    );
    // Retrieve Entity: relA{name} reaches b first, the bare relB after it
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened_projected(&building_a_id)
        .join_level(1.try_into()?)
        .pick("id,relA{name},relB".parse::<Projection>()?)
        .send()
        .await?;
    let entities_2 = retrieved.body();
    expect_that!(entities_2.as_slice(), len(eq(2)));
    expect_that!(entities_2.get(1).map(projected_member_count), some(eq(1)));
    let item_2 = entities_2.get(1).or_fail()?;
    expect_that!(item_2.id, none());
    expect_that!(entities_2.as_slice(), nth(0, has_id(&building_a_id)));
    expect_that!(entities_2.as_slice(), nth(1, has_attribute("name", any_attribute().value("Depot"))));
    // two groups: the first decides, and relB{id} adds no id
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened_projected(&building_a_id)
        .join_level(1.try_into()?)
        .pick("id,relA{name},relB{id}".parse::<Projection>()?)
        .send()
        .await?;
    let entities_3 = retrieved_2.body();
    expect_that!(entities_3.as_slice(), len(eq(2)));
    expect_that!(entities_3.get(1).map(projected_member_count), some(eq(1)));
    let item_3 = entities_3.get(1).or_fail()?;
    expect_that!(item_3.id, none());
    expect_that!(entities_3.as_slice(), nth(1, has_attribute("name", any_attribute().value("Depot"))));
    // the bare relA reaches b first, so b is appended whole and relB{name} does not narrow it
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened_projected(&building_a_id)
        .join_level(1.try_into()?)
        .pick("id,relA,relB{name}".parse::<Projection>()?)
        .send()
        .await?;
    let entities_4 = retrieved_3.body();
    expect_that!(entities_4.as_slice(), len(eq(2)));
    expect_that!(entities_4.get(1).map(projected_member_count), some(eq(4)));
    expect_that!(
        entities_4.as_slice(),
        nth(
            1,
            all!(
                has_id(&building_b_id),
                has_attribute("category", any_attribute().value("commercial")),
                has_attribute("name", any_attribute().value("Depot")),
                has_type(&building_type)
            )
        )
    );
    // the pick list names relB first: relB{category} decides, the bare relA does not widen it
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .ids(vec![building_a_id.clone()])
        .entity_type(&building_type)
        .join(Join::Flat)
        .join_level(1.try_into()?)
        .pick("relB{category},relA,id".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items().get(1).map(projected_member_count), some(eq(1)));
    expect_that!(
        page_2.items(),
        nth(1, all!(has_attribute("category", any_attribute().value("commercial")), lacks_attribute("name")))
    );
    // Retrieve Entity: relB{name} first, the bare relA after it
    let retrieved_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened_projected(&building_a_id)
        .join_level(1.try_into()?)
        .pick("id,relB{name},relA".parse::<Projection>()?)
        .send()
        .await?;
    let entities_5 = retrieved_4.body();
    expect_that!(entities_5.as_slice(), len(eq(2)));
    expect_that!(entities_5.get(1).map(projected_member_count), some(eq(1)));
    expect_that!(entities_5.as_slice(), nth(1, has_attribute("name", any_attribute().value("Depot"))));
    Ok(())
}
