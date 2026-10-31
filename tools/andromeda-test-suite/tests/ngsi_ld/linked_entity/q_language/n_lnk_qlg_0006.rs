//! A hop over several instances matches when any instance does.

use andromeda_test_suite::{broker, entity_matcher::has_instances, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Relationship},
    query::{Join, q},
};

/// Pins: a q hop over a Relationship with several `datasetId` instances selects the root when any instance names an
/// entity that satisfies it, and not when every instance misses
/// Clause: 4.9, 4.5.5, 4.5.23, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_lnk_qlg_0006_a_hop_over_several_instances_matches_when_any_instance_does() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_commercial_id = namespace.urn("Building", "commercial")?;
    let building_type = namespace.entity_type("Building")?;
    let building_industrial_id = namespace.urn("Building", "industrial")?;
    let building_depot_id = namespace.urn("Building", "depot")?;
    let car_mixed_id = namespace.urn("Car", "mixed")?;
    let car_type = namespace.entity_type("Car")?;
    let car_neither_id = namespace.urn("Car", "neither")?;
    let entities = [
        Entity::builder()
            .id(building_commercial_id.clone())
            .entity_type(building_type.clone())
            .property("category", "commercial")
            .build()?,
        Entity::builder()
            .id(building_industrial_id.clone())
            .entity_type(building_type.clone())
            .property("category", "industrial")
            .build()?,
        Entity::builder()
            .id(building_depot_id.clone())
            .entity_type(building_type.clone())
            .property("category", "industrial")
            .build()?,
        Entity::builder()
            .id(car_mixed_id.clone())
            .entity_type(car_type.clone())
            .relationship(
                "isParkedIn",
                Relationship::builder(building_commercial_id.clone()).dataset_id(namespace.dataset_id("Dataset", "first")?),
            )
            .relationship(
                "isParkedIn",
                Relationship::builder(building_industrial_id.clone()).dataset_id(namespace.dataset_id("Dataset", "second")?),
            )
            .build()?,
        Entity::builder()
            .id(car_neither_id)
            .entity_type(car_type.clone())
            .relationship(
                "isParkedIn",
                Relationship::builder(building_industrial_id.clone()).dataset_id(namespace.dataset_id("Dataset", "first")?),
            )
            .relationship(
                "isParkedIn",
                Relationship::builder(building_depot_id.clone()).dataset_id(namespace.dataset_id("Dataset", "second")?),
            )
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // both instances are held, so the selection below cannot pass by one instance collapsing into the other
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&car_mixed_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_instances("isParkedIn", 2));
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
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&car_mixed_id)));
    Ok(())
}
