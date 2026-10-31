//! Version 1 0 removes the scope.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    received_status::ExpectedStatus,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::{Accept, Prefer, SpecVersion},
    model::{Entity, NgsiLdMediaType, Value},
};

/// Pins: Retrieve Entity under Prefer ngsi-ld=1.0 removes the scope of the entity, which arrived at version 1.4,
/// and answers 203 with the version echoed
/// Clause: 4.3.6.8, 6.3.6, 5.7.1.4, 6.5.3.1
#[gtest]
#[tokio::test]
async fn n_cmn_prf_0010_version_1_0_removes_the_scope() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .scope(["/A/B"])
        .property("temperature", Value::Number("25.0".parse()?))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .with_prefer(Prefer::builder().version("1.0".parse::<SpecVersion>()?).build()?)
        .entities()
        .retrieve(&building_id)
        .send()
        .await?
        .expecting(StatusCode::NON_AUTHORITATIVE_INFORMATION)?;
    expect_that!(retrieved.preference_applied(), some(eq(&"1.0".parse::<SpecVersion>()?)));
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&building_id));
    expect_that!(entity.scope.as_ref(), none());
    expect_that!(entity, has_attribute("temperature", any_attribute().value(25)));
    Ok(())
}
