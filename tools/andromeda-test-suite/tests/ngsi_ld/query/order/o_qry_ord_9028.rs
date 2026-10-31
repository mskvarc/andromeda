//! Order by an attribute created at follows the stamp of that attribute.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::ordering::OrderBy,
};

/// Pins: Query Entities of two Vehicles, the second created first, with `orderBy` `name.createdAt` answers the
/// earlier-created one first
/// Clause: 4.8, 4.23.3
/// Recorded: clause 4.23.3 orders by the members a path names and leaves open whether a path reaches the
/// `createdAt` of an Attribute; Andromeda reads `name.createdAt` as the `createdAt` of the name Attribute
#[gtest]
#[tokio::test]
async fn o_qry_ord_9028_order_by_an_attribute_created_at_follows_the_stamp_of_that_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    // The Entities are created in the reverse of their identifier order, so an ordering by identifier cannot pass for one by time.
    let vehicle_2 = Entity::builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .property("name", "e")
        .build()?;
    broker.entities().create(&vehicle_2).send().await?.done()?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("name", "l")
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .ids(vec![vehicle_id.clone(), vehicle_2_id.clone()])
        .ordering("name.createdAt".parse::<OrderBy>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), nth(0, has_id(&vehicle_2_id)));
    expect_that!(page.items(), nth(1, has_id(&vehicle_id)));
    Ok(())
}
