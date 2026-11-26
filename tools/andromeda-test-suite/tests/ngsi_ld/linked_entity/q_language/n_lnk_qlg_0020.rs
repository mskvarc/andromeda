//! Each hop of a chain takes a type list of its own, which types the Entity that hop reaches.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::{Join, q},
};

/// Pins: over Cars parked in a Building owned by the Organization ACME, in a Building owned by the Person ACME and in
/// a Warehouse owned by the Organization ACME, Query Entities with q isParkedIn{Building:owner{name}}=="ACME" selects
/// the two Cars parked in a Building, and q isParkedIn{Building:owner{Organization:name}}=="ACME" selects only the
/// Car parked in the Building the Organization owns
/// Clause: 4.9, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_lnk_qlg_0020_each_hop_of_a_chain_takes_a_type_list_of_its_own() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let organization_id = namespace.urn("Organization", "acme")?;
    let organization_type = namespace.entity_type("Organization")?;
    let person_id = namespace.urn("Person", "acme")?;
    let building_type = namespace.entity_type("Building")?;
    let organization_building_id = namespace.urn("Building", "organization")?;
    let person_building_id = namespace.urn("Building", "person")?;
    let warehouse_id = namespace.urn("Warehouse", "organization")?;
    let car_type = namespace.entity_type("Car")?;
    let in_organization_building = namespace.urn("Car", "in-organization-building")?;
    let in_person_building = namespace.urn("Car", "in-person-building")?;
    let parked = |id, parked_in| -> Result<Entity<_>> {
        Ok(Entity::builder()
            .id(id)
            .entity_type(car_type.clone())
            .relationship("isParkedIn", parked_in)
            .build()?)
    };
    let entities = [
        Entity::builder()
            .id(organization_id.clone())
            .entity_type(organization_type.clone())
            .property("name", "ACME")
            .build()?,
        Entity::builder()
            .id(person_id.clone())
            .entity_type(namespace.entity_type("Person")?)
            .property("name", "ACME")
            .build()?,
        Entity::builder()
            .id(organization_building_id.clone())
            .entity_type(building_type.clone())
            .relationship("owner", organization_id.clone())
            .build()?,
        Entity::builder()
            .id(person_building_id.clone())
            .entity_type(building_type.clone())
            .relationship("owner", person_id)
            .build()?,
        Entity::builder()
            .id(warehouse_id.clone())
            .entity_type(namespace.entity_type("Warehouse")?)
            .relationship("owner", organization_id)
            .build()?,
        parked(in_organization_building.clone(), organization_building_id)?,
        parked(in_person_building.clone(), person_building_id)?,
        parked(namespace.urn("Car", "in-warehouse")?, warehouse_id)?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let outer_typed = json
        .entities()
        .query()
        .entity_type(&car_type)
        .q(q::attr("isParkedIn").linked_typed([&building_type], "owner").linked("name").eq("ACME"))
        .join(Join::Inline)
        .join_level(2.try_into()?)
        .send()
        .await?;
    expect_that!(
        outer_typed.items(),
        unordered_elements_are![has_id(&in_organization_building), has_id(&in_person_building)]
    );
    let both_typed = json
        .entities()
        .query()
        .entity_type(&car_type)
        .q(q::attr("isParkedIn")
            .linked_typed([&building_type], "owner")
            .linked_typed([&organization_type], "name")
            .eq("ACME"))
        .join(Join::Inline)
        .join_level(2.try_into()?)
        .send()
        .await?;
    expect_that!(both_typed.items(), unordered_elements_are![has_id(&in_organization_building)]);
    Ok(())
}
