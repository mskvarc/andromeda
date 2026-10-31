//! An entity map opens with the context.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
};

/// Pins: Retrieve `EntityMap` answered as application/ld+json opens with @context
/// Clause: 6.3.6, 5.14.1, 5.14.4
/// Recorded: 6.3.6 requires the @context in an application/ld+json body and fixes no member order (RFC 8259 objects
/// are unordered); Andromeda writes it as the first member, of every item of a collection too
#[gtest]
#[tokio::test]
async fn o_cmn_cnt_9003_an_entity_map_opens_with_the_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder().id(vehicle_id).entity_type(vehicle_type.clone()).build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let ld_json = broker.with_responses().with_accept(Accept::new(NgsiLdMediaType::JsonLd));
    let created = ld_json
        .entity_maps()
        .create()
        .entity_type(&vehicle_type)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map_id = &created.body().id;
    let retrieved = ld_json.entity_maps().retrieve(map_id).send().await?;
    expect_that!(&String::from_utf8_lossy(&retrieved.body).into_owned(), contains_regex(r#"^\s*\{\s*"@context""#));
    broker.entity_maps().delete(map_id).send().await?;
    Ok(())
}
