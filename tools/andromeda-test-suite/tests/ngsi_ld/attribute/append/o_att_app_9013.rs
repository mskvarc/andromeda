//! A prefixed attribute name breaking the grammar after the colon is bad request data.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::{has_attribute, member_count},
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: Append Attributes naming ex:bad-name under an @context defining the prefix ex answers 400 `BadRequestData` and
/// leaves the entity as it was
/// Clause: 4.6.2, 5.6.3.4
/// Recorded: clause 4.6.2 says an implementation "should" raise `BadRequestData` for a name outside its grammar;
/// Andromeda refuses a name the request's @context reads as a compact IRI whose part after the colon breaks it
#[gtest]
#[tokio::test]
async fn o_att_app_9013_a_prefixed_attribute_name_breaking_the_grammar_after_the_colon_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_append_prefixed_id = namespace.urn("Vehicle", "append-prefixed")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_append_prefixed = Entity::builder()
        .id(vehicle_append_prefixed_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 1)
        .build()?;
    broker.entities().create(&vehicle_append_prefixed).send().await?.done()?;
    // The attribute name is the point of the test and the model refuses it, so the body is sent as written.
    let request = request_to(&broker, Method::POST, &format!("entities/{vehicle_append_prefixed_id}/attrs"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
        .body(Bytes::from(
            r#"{"ex:bad-name": {"type": "Property", "value": 2}, "@context": {"ex": "http://example.org/"}}"#,
        ));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_append_prefixed_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", property().value(1)));
    expect_that!(member_count(entity), eq(3));
    Ok(())
}
