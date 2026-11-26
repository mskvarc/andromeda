//! A hop with a type list over several `datasetId` instances types the object of each instance.

use andromeda_test_suite::{broker, entity_matcher::has_instances, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Relationship},
    query::{Join, q},
};

/// Pins: over a Car whose Relationship isParkedIn has an instance naming a commercial Warehouse and one naming a
/// commercial Building, and a Car whose instances name an industrial Building and the commercial Warehouse, Query
/// Entities with q isParkedIn{category}=="commercial" selects both, and q isParkedIn{Building:category}=="commercial"
/// selects only the first, since only its Building instance names a linked Entity of the listed type that holds the
/// value
/// Clause: 4.5.5, 4.5.23, 4.9, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_lnk_qlg_0023_a_hop_with_a_type_list_over_several_instances_types_the_object_of_each() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let commercial_building_id = namespace.urn("Building", "commercial")?;
    let industrial_building_id = namespace.urn("Building", "industrial")?;
    let commercial_warehouse_id = namespace.urn("Warehouse", "commercial")?;
    let car_type = namespace.entity_type("Car")?;
    let through_building = namespace.urn("Car", "through-building")?;
    let through_warehouse = namespace.urn("Car", "through-warehouse")?;
    let first = namespace.dataset_id("Dataset", "first")?;
    let second = namespace.dataset_id("Dataset", "second")?;
    let place =
        |id, place_type, category| -> Result<Entity<_>> { Ok(Entity::builder().id(id).entity_type(place_type).property("category", category).build()?) };
    let car = |id, first_object, second_object| -> Result<Entity<_>> {
        Ok(Entity::builder()
            .id(id)
            .entity_type(car_type.clone())
            .relationship("isParkedIn", Relationship::builder(first_object).dataset_id(first.clone()))
            .relationship("isParkedIn", Relationship::builder(second_object).dataset_id(second.clone()))
            .build()?)
    };
    let entities = [
        place(commercial_building_id.clone(), building_type.clone(), "commercial")?,
        place(industrial_building_id.clone(), building_type.clone(), "industrial")?,
        place(commercial_warehouse_id.clone(), namespace.entity_type("Warehouse")?, "commercial")?,
        car(through_building.clone(), commercial_warehouse_id.clone(), commercial_building_id)?,
        car(through_warehouse.clone(), industrial_building_id, commercial_warehouse_id)?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    // Both instances are held, so neither selection can pass by one instance collapsing into the other.
    for car_id in [&through_building, &through_warehouse] {
        let retrieved = json.entities().retrieve(car_id).send().await?;
        expect_that!(retrieved.body(), has_instances("isParkedIn", 2), "{car_id}");
    }
    let untyped = json
        .entities()
        .query()
        .entity_type(&car_type)
        .q(q::attr("isParkedIn").linked("category").eq("commercial"))
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(untyped.items(), unordered_elements_are![has_id(&through_building), has_id(&through_warehouse)]);
    let typed = json
        .entities()
        .query()
        .entity_type(&car_type)
        .q(q::attr("isParkedIn").linked_typed([&building_type], "category").eq("commercial"))
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(typed.items(), unordered_elements_are![has_id(&through_building)]);
    Ok(())
}
