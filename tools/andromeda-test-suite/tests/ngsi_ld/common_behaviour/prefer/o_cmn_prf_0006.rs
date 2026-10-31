//! An older version on query is answered 203 and echoed.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::{Accept, Prefer, SpecVersion},
    model::{Entity, NgsiLdMediaType, Value},
};

/// Pins: Query Entities with Prefer ngsi-ld=1.4 on entities that need no amendment is answered 203
/// Non-Authoritative Information and Preference-Applied names version 1.4
/// Clause: 6.3.6, 4.3.6.8, 5.7.2.4, 6.4.3.2
/// Recorded: 5.7.2.4 and 6.3.6 require the Preference-Applied header for a version preference, but Table 6.4.3.2-2
/// gives 203 to "an altered response body"; whether a body that needed no change is answered 200 or 203 is left
/// open, and Andromeda answers 203
#[gtest]
#[tokio::test]
async fn o_cmn_prf_0006_an_older_version_on_query_is_answered_203_and_echoed() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("temperature", Value::Number("25.0".parse()?))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let page = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .with_prefer(Prefer::builder().version("1.4".parse::<SpecVersion>()?).build()?)
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_id.clone()])
        .send()
        .await?
        .expecting(StatusCode::NON_AUTHORITATIVE_INFORMATION)?;
    expect_that!(page.preference_applied(), some(eq(&"1.4".parse::<SpecVersion>()?)));
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&building_id)));
    Ok(())
}
