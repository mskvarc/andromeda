//! A dotted created at path orders by the attribute creation time.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::{Locality, ordering::OrderBy},
};

/// Pins: Query Entities with `orderBy` `name.createdAt` answers 200 with the three Entities in the order their name
/// Attributes were created
/// Clause: 4.23.3, 4.8
#[gtest]
#[tokio::test]
async fn o_qry_ord_9003_a_dotted_created_at_path_orders_by_the_attribute_creation_time() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let timestamped_c_id = namespace.urn("Timestamped", "c")?;
    let timestamped_type = namespace.entity_type("Timestamped")?;
    let timestamped_b_id = namespace.urn("Timestamped", "b")?;
    let timestamped_a_id = namespace.urn("Timestamped", "a")?;
    // The Entities are created in the reverse of their identifier order, so an ordering by identifier cannot pass for one by time.
    let timestamped_c = Entity::builder()
        .id(timestamped_c_id.clone())
        .entity_type(timestamped_type.clone())
        .property("name", "value for c")
        .build()?;
    broker.entities().create(&timestamped_c).send().await?.done()?;
    let timestamped_b = Entity::builder()
        .id(timestamped_b_id.clone())
        .entity_type(timestamped_type.clone())
        .property("name", "value for b")
        .build()?;
    broker.entities().create(&timestamped_b).send().await?.done()?;
    let timestamped_a = Entity::builder()
        .id(timestamped_a_id.clone())
        .entity_type(timestamped_type.clone())
        .property("name", "value for a")
        .build()?;
    broker.entities().create(&timestamped_a).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&timestamped_type)
        .locality(Locality::Local)
        .ordering("name.createdAt".parse::<OrderBy>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(3)));
    expect_that!(page.items(), nth(0, has_id(&timestamped_c_id)));
    expect_that!(page.items(), nth(1, has_id(&timestamped_b_id)));
    expect_that!(page.items(), nth(2, has_id(&timestamped_a_id)));
    Ok(())
}
