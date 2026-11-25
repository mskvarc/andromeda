//! A linked Entity counts when one of its types is the listed one.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::{Join, q},
};

/// Pins: Query Entities with q isParkedIn{Building:category}=="commercial" selects the Car parked in an Entity typed
/// both Building and Warehouse
/// Clause: 4.9, 4.17, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_lnk_qlg_0018_a_linked_entity_counts_when_one_of_its_types_is_the_listed_one() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let depot_id = namespace.urn("Depot", "both")?;
    let building_type = namespace.entity_type("Building")?;
    let car_type = namespace.entity_type("Car")?;
    let car_id = namespace.urn("Car", "in-depot")?;
    let entities = [
        Entity::builder()
            .id(depot_id.clone())
            .entity_type(vec![building_type.clone(), namespace.entity_type("Warehouse")?])
            .property("category", "commercial")
            .build()?,
        Entity::builder()
            .id(car_id.clone())
            .entity_type(car_type.clone())
            .relationship("isParkedIn", depot_id)
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&car_type)
        .q(q::attr("isParkedIn").linked_typed([&building_type], "category").eq("commercial"))
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&car_id)));
    Ok(())
}
