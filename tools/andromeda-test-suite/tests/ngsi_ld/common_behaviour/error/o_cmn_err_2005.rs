//! Replace attribute with an unknown parameter is invalid request.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    wire::api_uri,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::ReplaceAttribute},
    model::{Attribute, AttributeName, Entity, ErrorType, NgsiLdMediaType, Property},
};

/// Pins: Replace Attribute with a URL parameter the operation does not define answers `InvalidRequest` and replaces
/// nothing
/// Clause: 6.3.20, 5.6.19
/// Recorded: 6.3.20 says an incompatible parameter "should" give `InvalidRequest`
#[gtest]
#[tokio::test]
async fn o_cmn_err_2005_replace_attribute_with_an_unknown_parameter_is_invalid_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let device = Entity::builder().id(device_id.clone()).entity_type(device_type).property("speed", 1).build()?;
    broker.entities().create(&device).send().await?.done()?;
    let attribute = Attribute::from(Property::builder(2).build()?);
    // The operation defines no `invalidParam` parameter; the request carries it as written.
    let request = broker
        .unchecked(
            &ReplaceAttribute::builder()
                .id(&device_id)
                .name(&"speed".parse::<AttributeName>()?)
                .attribute(&attribute)
                .build(),
        )?
        .uri(api_uri(&broker, &format!("entities/{device_id}/attrs/speed?invalidParam=invalidValue"))?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(1)));
    Ok(())
}
