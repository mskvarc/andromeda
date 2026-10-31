//! A request without the header gets no preference applied.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Value},
};

/// Pins: Retrieve Entity without a Prefer header is answered 200 and carries no Preference-Applied header
/// Clause: 6.3.6, 5.7.1.4
/// Recorded: neither 6.3.6 nor IETF RFC 7240 (section 3: the header "MAY be included" to name the honoured
/// preferences) forbids the header when no preference was sent; Andromeda sends none
#[gtest]
#[tokio::test]
async fn o_cmn_prf_0007_a_request_without_the_header_gets_no_preference_applied() -> Result<()> {
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
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    expect_that!(retrieved.preference_applied(), none());
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&building_id));
    Ok(())
}
