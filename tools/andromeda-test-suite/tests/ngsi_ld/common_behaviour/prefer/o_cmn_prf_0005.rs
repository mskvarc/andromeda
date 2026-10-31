//! An older version on retrieve is answered 203 and echoed.

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

/// Pins: Retrieve Entity with Prefer ngsi-ld=1.3 on an entity that needs no amendment is answered 203
/// Non-Authoritative Information and Preference-Applied names version 1.3
/// Clause: 6.3.6, 4.3.6.8, 5.7.1.4, 6.5.3.1
/// Recorded: 5.7.1.4 and 6.3.6 require the Preference-Applied header for a version preference, but Table 6.5.3.1-3
/// gives 203 to "an altered response body"; whether a body that needed no change is answered 200 or 203 is left
/// open, and Andromeda answers 203
#[gtest]
#[tokio::test]
async fn o_cmn_prf_0005_an_older_version_on_retrieve_is_answered_203_and_echoed() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .property("temperature", Value::Number("25.0".parse()?))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .with_prefer(Prefer::builder().version("1.3".parse::<SpecVersion>()?).build()?)
        .entities()
        .retrieve(&building_id)
        .send()
        .await?
        .expecting(StatusCode::NON_AUTHORITATIVE_INFORMATION)?;
    expect_that!(retrieved.preference_applied(), some(eq(&"1.3".parse::<SpecVersion>()?)));
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&building_id));
    expect_that!(entity, has_attribute("temperature", any_attribute().value(25)));
    Ok(())
}
