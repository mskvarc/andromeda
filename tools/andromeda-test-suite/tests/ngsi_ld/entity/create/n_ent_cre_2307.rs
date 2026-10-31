//! An expires at with a comma or six fraction digits is accepted.

use andromeda_test_suite::{broker, namespace::Namespace, problem::answered};
use bytes::Bytes;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::CreateEntity},
    model::{DateTime, Entity, NgsiLdMediaType},
};

/// Pins: Create Entity accepts an `expiresAt` written with a comma fraction or with six fractional digits, on the
/// Entity, on an Attribute and on a sub-attribute, and answers each as the instant it names
/// Clause: 4.6.3, 4.22, 5.6.1, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_ent_cre_2307_an_expires_at_with_a_comma_or_six_fraction_digits_is_accepted() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder().id(building_id.clone()).entity_type(building_type).build()?;
    let body = format!(
        r#"{{"id": "urn:ngsi-ld:Building:{namespace}-1", "type": "Building_{namespace}", "expiresAt": "2099-01-01T00:00:00,5Z", "temperature": {{"type": "Property", "value": 21, "expiresAt": "2099-01-01T00:00:00.123456Z", "accuracy": {{ "type": "Property", "value": 0.5, "expiresAt": "2099-01-01T00:00:00,250Z" }}}}}}"#
    );
    // The model writes the comma fraction with a point, which is not the spelling under test, so the body is sent as written.
    let request = broker.unchecked(&CreateEntity::builder().entity(&building).build())?.body(Bytes::from(body));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.expires_at.as_ref(), some(eq(&"2099-01-01T00:00:00.500Z".parse::<DateTime>()?)));
    let temperature = entity
        .attributes
        .iter()
        .find(|(name, _)| name.as_str() == "temperature")
        .map(|(_, instances)| instances.first())
        .or_fail()?;
    expect_that!(
        temperature.meta().expires_at.as_ref(),
        some(eq(&"2099-01-01T00:00:00.123456Z".parse::<DateTime>()?))
    );
    let accuracy = temperature
        .meta()
        .attributes
        .iter()
        .find(|(name, _)| name.as_str() == "accuracy")
        .map(|(_, instances)| instances.first())
        .or_fail()?;
    expect_that!(accuracy.meta().expires_at.as_ref(), some(eq(&"2099-01-01T00:00:00.250Z".parse::<DateTime>()?)));
    Ok(())
}
