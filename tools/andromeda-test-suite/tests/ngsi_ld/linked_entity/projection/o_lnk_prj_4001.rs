//! The pick list decides a shared flat target whichever relationship the entity stores first.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute, projected_member_count},
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::{Join, projection::Projection},
};

/// Pins: under a flat join a target that two picked Relationships share is projected by the Relationship that comes
/// first in the pick list also when the entity was created with its Relationships in the other order, on Query
/// Entities and on Retrieve Entity
/// Clause: 4.21, 4.5.23.3, 5.7.1.5, 5.7.2.5
/// Recorded: 5.7.1.5 and 5.7.2.5 append a target "unless a target URI has been previously encountered" and 4.21
/// addresses a Linked Entity Term to the Linked Entity reached through its Relationship, but no clause says which
/// Relationship's projection applies when two reach the same appended target, so the file is kind O; the pick list
/// is the only order the request defines, an entity's members having none in JSON-LD. Ruled by the user
/// (2026-10-05)
#[gtest]
#[tokio::test]
async fn o_lnk_prj_4001_the_pick_list_decides_a_shared_flat_target_whichever_relationship_the_entity_stores_first() -> Result<()> {
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
            .relationship("relB", building_b_id.clone())
            .relationship("relA", building_b_id.clone())
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // the entity stores relB first and the pick list names relA{category} first: the group of the pick list decides and the bare relB does not widen it
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
    expect_that!(page.items(), nth(0, has_id(&building_a_id)));
    expect_that!(
        page.items(),
        nth(1, all!(has_attribute("category", any_attribute().value("commercial")), lacks_attribute("name")))
    );
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
    expect_that!(entities_2.as_slice(), nth(0, has_id(&building_a_id)));
    expect_that!(entities_2.as_slice(), nth(1, has_attribute("name", any_attribute().value("Depot"))));
    // the bare relA comes first in the pick list: b is appended whole although relB, which the entity stores first, carries a group
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .ids(vec![building_a_id.clone()])
        .entity_type(&building_type)
        .join(Join::Flat)
        .join_level(1.try_into()?)
        .pick("relA,relB{category},id".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items().get(1).map(projected_member_count), some(eq(4)));
    expect_that!(
        page_2.items(),
        nth(
            1,
            all!(
                has_id(&building_b_id),
                has_attribute("category", any_attribute().value("commercial")),
                has_attribute("name", any_attribute().value("Depot"))
            )
        )
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened_projected(&building_a_id)
        .join_level(1.try_into()?)
        .pick("id,relA,relB{name}".parse::<Projection>()?)
        .send()
        .await?;
    let entities_3 = retrieved_2.body();
    expect_that!(entities_3.as_slice(), len(eq(2)));
    expect_that!(entities_3.get(1).map(projected_member_count), some(eq(4)));
    expect_that!(
        entities_3.as_slice(),
        nth(1, all!(has_id(&building_b_id), has_attribute("category", any_attribute().value("commercial"))))
    );
    Ok(())
}
