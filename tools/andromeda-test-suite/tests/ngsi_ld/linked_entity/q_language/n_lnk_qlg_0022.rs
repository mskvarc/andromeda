//! A hop with a type list over a `ListRelationship` types each object the list names.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ListRelationship, NgsiLdMediaType},
    query::{Join, q},
};

/// Pins: over Cars whose `ListRelationship` visited names a commercial Warehouse and a commercial Building, the
/// commercial Warehouse alone, and an industrial Building and the commercial Warehouse, Query Entities with q
/// visited{category}=="commercial" selects all three, and q visited{Building:category}=="commercial" selects only the
/// first, since the commercial Warehouse the others name is of no listed type
/// Clause: 4.5.22, 4.5.23, 4.9, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_lnk_qlg_0022_a_hop_with_a_type_list_over_a_list_relationship_types_each_object() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let commercial_building_id = namespace.urn("Building", "commercial")?;
    let industrial_building_id = namespace.urn("Building", "industrial")?;
    let commercial_warehouse_id = namespace.urn("Warehouse", "commercial")?;
    let car_type = namespace.entity_type("Car")?;
    let through_building = namespace.urn("Car", "through-building")?;
    let warehouse_only = namespace.urn("Car", "warehouse-only")?;
    let through_warehouse = namespace.urn("Car", "through-warehouse")?;
    let place =
        |id, place_type, category| -> Result<Entity<_>> { Ok(Entity::builder().id(id).entity_type(place_type).property("category", category).build()?) };
    let car = |id, visited| -> Result<Entity<_>> {
        Ok(Entity::builder()
            .id(id)
            .entity_type(car_type.clone())
            .list_relationship("visited", ListRelationship::builder(visited))
            .build()?)
    };
    let entities = [
        place(commercial_building_id.clone(), building_type.clone(), "commercial")?,
        place(industrial_building_id.clone(), building_type.clone(), "industrial")?,
        place(commercial_warehouse_id.clone(), namespace.entity_type("Warehouse")?, "commercial")?,
        car(through_building.clone(), vec![commercial_warehouse_id.clone(), commercial_building_id])?,
        car(warehouse_only.clone(), vec![commercial_warehouse_id.clone()])?,
        car(through_warehouse.clone(), vec![industrial_building_id, commercial_warehouse_id])?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let untyped = json
        .entities()
        .query()
        .entity_type(&car_type)
        .q(q::attr("visited").linked("category").eq("commercial"))
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(
        untyped.items(),
        unordered_elements_are![has_id(&through_building), has_id(&warehouse_only), has_id(&through_warehouse)]
    );
    let typed = json
        .entities()
        .query()
        .entity_type(&car_type)
        .q(q::attr("visited").linked_typed([&building_type], "category").eq("commercial"))
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(typed.items(), unordered_elements_are![has_id(&through_building)]);
    Ok(())
}
