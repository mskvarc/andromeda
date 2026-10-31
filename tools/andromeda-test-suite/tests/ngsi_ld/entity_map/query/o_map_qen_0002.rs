//! A created map is located under entity maps.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::{Accept, Location},
    model::{Entity, NgsiLdMediaType, Property},
};

/// Pins: Create `EntityMap` for Query Entities answers 201 with a Location naming the new `EntityMap` under
/// `/ngsi-ld/v1/entityMaps/`
/// Clause: 5.14.4.5, 6.34.3.1
/// Recorded: 6.34.3.1 requires the `NGSILD-EntityMap` header only; Andromeda also answers the 201 with a relative
/// Location of the `EntityMap` resource
#[gtest]
#[tokio::test]
async fn o_map_qen_0002_a_created_map_is_located_under_entity_maps() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(50).observed_at("2021-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().batch_create(&[vehicle]).send().await?.done()?;
    let created = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(created.status, eq(StatusCode::CREATED));
    let map_id = created.outcome.body().id.clone();
    let expected = format!("/ngsi-ld/v1/entityMaps/{map_id}");
    expect_that!(
        created.head.typed::<Location>().valid().map(|location| location.as_iri_ref().as_str()),
        some(eq(expected.as_str()))
    );
    broker.entity_maps().delete(&map_id).send().await?;
    Ok(())
}
