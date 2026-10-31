//! Q finds the empty array by existence and never by equality.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Value},
    query::q,
};

/// Pins: Query Entities with q=tags answers both an entity whose tags is the empty array and one whose tags is
/// `["a"]`, and with q=tags=="a" only the second
/// Clause: 4.9, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9001_q_finds_the_empty_array_by_existence_and_never_by_equality() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let empty_probe_emptied_id = namespace.urn("EmptyProbe", "emptied")?;
    let empty_probe_type = namespace.entity_type("EmptyProbe")?;
    let empty_probe_tagged_id = namespace.urn("EmptyProbe", "tagged")?;
    let entities = [
        Entity::builder()
            .id(empty_probe_emptied_id.clone())
            .entity_type(empty_probe_type.clone())
            .property("tags", Value::Array(vec![]))
            .build()?,
        Entity::builder()
            .id(empty_probe_tagged_id.clone())
            .entity_type(empty_probe_type.clone())
            .property("tags", Value::Array(vec![Value::from("a")]))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&empty_probe_type)
        .ids(vec![empty_probe_emptied_id.clone(), empty_probe_tagged_id.clone()])
        .q(q::attr("tags").exists())
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&empty_probe_emptied_id)));
    expect_that!(page.items(), contains(has_id(&empty_probe_tagged_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&empty_probe_type)
        .ids(vec![empty_probe_emptied_id.clone(), empty_probe_tagged_id.clone()])
        .q(q::attr("tags").eq("a"))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&empty_probe_tagged_id)));
    Ok(())
}
