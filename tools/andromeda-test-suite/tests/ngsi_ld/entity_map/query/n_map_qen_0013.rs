//! A query without entity map answers 200 without the header.

use andromeda_test_suite::{broker, header_matcher::absent, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property},
};

/// Pins: Query Entities without `entityMap` answers 200 with no `NGSILD-EntityMap` header
/// Clause: 6.4.3.2, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_map_qen_0013_a_query_without_entity_map_answers_200_without_the_header() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let entities = [Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(50).observed_at("2021-01-01T00:00:00Z".parse()?))
        .build()?];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page.entity_map(), absent());
    expect_that!(page.items(), nth(0, has_id(&vehicle_id)));
    Ok(())
}
