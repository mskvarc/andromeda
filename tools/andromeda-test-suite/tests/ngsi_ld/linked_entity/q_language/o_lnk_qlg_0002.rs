//! An entity type list in a hop restricts the linked entity.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::{Join, q},
};

/// Pins: an Entity Type list before the colon in a q hop restricts the Linked Entity to those types: a target of
/// another type does not admit its root even when its attribute matches
/// Clause: 4.9, 4.5.23
/// Recorded: clause 4.9 EXAMPLE 14 writes the list as a "hint for the required Entity Type `[...]` so only such
/// NGSI-LD Entities need to be considered", and its ABNF admits the list only before a further hop
/// (`LinkedEntityPath` = *1(EntityType 1*("," `EntityType`) ":") `AttrName` "{" `LinkedEntityPath` "}"), not before
/// the compared attribute as EXAMPLE 14 writes it; whether the hint must exclude a target of another type is left
/// open. Andromeda accepts the EXAMPLE 14 form and reads the list as a restriction
#[gtest]
#[tokio::test]
async fn o_lnk_qlg_0002_an_entity_type_list_in_a_hop_restricts_the_linked_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_listed_id = namespace.urn("Building", "listed")?;
    let building_type = namespace.entity_type("Building")?;
    let warehouse_unlisted_id = namespace.urn("Warehouse", "unlisted")?;
    let warehouse_type = namespace.entity_type("Warehouse")?;
    let car_to_building_id = namespace.urn("Car", "to-building")?;
    let car_type = namespace.entity_type("Car")?;
    let car_to_warehouse_id = namespace.urn("Car", "to-warehouse")?;
    let entities = [
        Entity::builder()
            .id(building_listed_id.clone())
            .entity_type(building_type.clone())
            .property("category", "commercial")
            .build()?,
        Entity::builder()
            .id(warehouse_unlisted_id.clone())
            .entity_type(warehouse_type)
            .property("category", "commercial")
            .build()?,
        Entity::builder()
            .id(car_to_building_id.clone())
            .entity_type(car_type.clone())
            .relationship("isParkedIn", building_listed_id.clone())
            .build()?,
        Entity::builder()
            .id(car_to_warehouse_id)
            .entity_type(car_type.clone())
            .relationship("isParkedIn", warehouse_unlisted_id.clone())
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // without the list both Cars are selected, so the list alone excludes the Warehouse-parked one below
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&car_type)
        .q(q::attr("isParkedIn").linked("category").eq("commercial"))
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    // the list names Building and Garage, so the Warehouse-parked Car is excluded
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&car_type)
        .q(q::attr("isParkedIn")
            .linked_typed([&building_type, &namespace.entity_type("Garage")?], "category")
            .eq("commercial"))
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&car_to_building_id)));
    Ok(())
}
