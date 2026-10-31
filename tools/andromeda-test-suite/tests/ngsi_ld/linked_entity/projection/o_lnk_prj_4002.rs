//! Three relationships to one flat target are projected by the first in the pick list.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::{has_attribute, projected_member_count},
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{AttributeName, Entity, NgsiLdMediaType, SimplifiedValue, Value},
    query::{Join, projection::Projection},
};

/// Pins: under a flat join three Relationships that reach one target are projected by the one that comes first in
/// the pick list, in the simplified representation as in the normalized one, whichever of them the entity stores
/// first
/// Clause: 4.21, 4.5.23.3, 4.5.1, 5.7.1.5, 5.7.2.5
/// Recorded: 5.7.1.5 and 5.7.2.5 append a target "unless a target URI has been previously encountered" and 4.21
/// addresses a Linked Entity Term to the Linked Entity reached through its Relationship, but no clause says which
/// Relationship's projection applies when several reach the same appended target, so the file is kind O; the pick
/// list is the only order the request defines, an entity's members having none in JSON-LD. Ruled by the user
/// (2026-10-05)
#[gtest]
#[tokio::test]
async fn o_lnk_prj_4002_three_relationships_to_one_flat_target_are_projected_by_the_first_in_the_pick_list() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let building_a_id = namespace.urn("Building", "a")?;
    let building_b_id = namespace.urn("Building", "b")?;
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
            .relationship("relC", building_b_id.clone())
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // relC{name} comes first in the pick list: it decides, and relA and relB, stored before it, do not widen it
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .entity_type(&building_type)
        .ids(vec![building_a_id.clone()])
        .join(Join::Flat)
        .join_level(1.try_into()?)
        .pick("relC{name},relA,relB,id".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), nth(0, has_id(&building_a_id)));
    expect_that!(page.items().get(1).map(projected_member_count), some(eq(1)));
    expect_that!(page.items(), nth(1, has_attribute("name", property().value("Depot"))));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected_simplified()
        .entity_type(&building_type)
        .ids(vec![building_a_id.clone()])
        .join(Join::Flat)
        .join_level(1.try_into()?)
        .pick("relB{category},relC{name},relA,id".parse::<Projection>()?)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items().first().and_then(|item| item.id.as_ref()), some(eq(&building_a_id)));
    let linked = page_2.items().get(1).or_fail()?;
    expect_that!(linked.attributes.len(), eq(1));
    expect_that!(linked.id.is_none(), eq(true));
    expect_that!(linked.entity_type.is_none(), eq(true));
    expect_that!(
        linked.attributes.get(&"category".parse::<AttributeName>()?),
        some(eq(&SimplifiedValue::Value(Value::from("commercial"))))
    );
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened_projected(&building_a_id)
        .join_level(1.try_into()?)
        .pick("id,relB,relC{name},relA{category}".parse::<Projection>()?)
        .send()
        .await?;
    let entities_3 = retrieved.body().as_slice();
    expect_that!(entities_3, len(eq(2)));
    expect_that!(entities_3.get(1).map(projected_member_count), some(eq(4)));
    expect_that!(entities_3, nth(1, has_id(&building_b_id)));
    expect_that!(entities_3, nth(1, has_attribute("name", property().value("Depot"))));
    Ok(())
}
