//! A relationship equality matches an object longer than the index bound.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::q,
};

/// Pins: a q equality on a Relationship whose one object is about 1,100 bytes long matches the entity holding that
/// object, and an operand differing in its last character does not
/// Clause: 4.9, 4.5.3
#[gtest]
#[tokio::test]
async fn n_qry_qlg_0036_a_relationship_equality_matches_an_object_longer_than_the_index_bound() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let machine_id = namespace.urn("Machine", 1)?;
    let machine_type = namespace.entity_type("Machine")?;
    let part_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx_id = namespace.urn("Part", "x".repeat(1074).as_str())?;
    let entities = [Entity::builder()
        .id(machine_id.clone())
        .entity_type(machine_type.clone())
        .relationship("hasPart", part_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx_id)
        .build()?];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&machine_type)
        .q(q::attr("hasPart").eq(format!("urn:ngsi-ld:Part:{namespace}-{}", "x".repeat(1074)).as_str()))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&machine_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&machine_type)
        .q(q::attr("hasPart").eq(format!("urn:ngsi-ld:Part:{namespace}-{}y", "x".repeat(1073)).as_str()))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    Ok(())
}
