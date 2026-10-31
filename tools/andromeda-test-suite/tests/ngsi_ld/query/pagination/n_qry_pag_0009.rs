//! A plain json query gets pointers of type application json.

use andromeda_test_suite::{broker, link_matcher::link_value, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::{Accept, Link, RelationType},
    model::{Entity, NgsiLdMediaType},
};

/// Pins: Query Entities as application/json with limit 2 and offset 0 over three entities answers a next pointer of
/// type application/json, not application/ld+json
/// Clause: 6.3.10
#[gtest]
#[tokio::test]
async fn n_qry_pag_0009_a_plain_json_query_gets_pointers_of_type_application_json() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    for n in 0..3 {
        let building = Entity::builder()
            .id(namespace.urn("Building", format!("{n:04}"))?)
            .entity_type(building_type.clone())
            .property("name", format!("b{n}"))
            .build()?;
        broker.entities().create(&building).send().await?.done()?;
    }
    let headed = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .limit(2)
        .offset(0)
        .send()
        .await?;
    expect_that!(
        headed.head.typed::<Link>().valid(),
        some(link_value(RelationType::Next).with_media_type("application/json"))
    );
    expect_that!(
        headed.head.typed::<Link>().valid(),
        not(some(link_value(RelationType::Next).with_media_type("application/ld+json")))
    );
    Ok(())
}
