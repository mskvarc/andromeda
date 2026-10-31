//! Entities are listed in the byte order of their identifiers.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
};

/// Pins: Query Entities of a type held by Entities whose identifiers end in b, a, Z and B answers them in the byte
/// order of their identifiers, B, Z, a, b, not in an alphabetical order folding case
/// Clause: 5.7.2, 4.23.1
/// Recorded: clause 5.7.2 sets no order on the Entities a query without `orderBy` answers; Andromeda answers them
/// in the order of their identifiers compared byte by byte, the order its cursor pages walk
#[gtest]
#[tokio::test]
async fn o_qry_pag_9015_entities_are_listed_in_the_byte_order_of_their_identifiers() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let item_b_id = namespace.urn("Item", "b")?;
    let item_type = namespace.entity_type("Item")?;
    let item_a_id = namespace.urn("Item", "a")?;
    let item_z_id = namespace.urn("Item", "Z")?;
    let item_b_id_2 = namespace.urn("Item", "B")?;
    let entities = [
        Entity::builder().id(item_b_id.clone()).entity_type(item_type.clone()).build()?,
        Entity::builder().id(item_a_id.clone()).entity_type(item_type.clone()).build()?,
        Entity::builder().id(item_z_id.clone()).entity_type(item_type.clone()).build()?,
        Entity::builder().id(item_b_id_2.clone()).entity_type(item_type.clone()).build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&item_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(4)));
    expect_that!(page.items(), nth(0, has_id(&item_b_id_2)));
    expect_that!(page.items(), nth(1, has_id(&item_z_id)));
    expect_that!(page.items(), nth(2, has_id(&item_a_id)));
    expect_that!(page.items(), nth(3, has_id(&item_b_id)));
    Ok(())
}
