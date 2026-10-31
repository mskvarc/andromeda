//! A fragment of an unknown attribute type is bad request data and replaces nothing.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::has_attribute,
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
    model::{Entity, ErrorType},
    query::Locality,
};

/// Pins: Replace Entity Attribute with a fragment whose type is not an NGSI-LD attribute type answers 400
/// `BadRequestData` and the Attribute is unchanged
/// Clause: 5.6.19.4, 5.2.5, 6.7.3.3
///
/// Invalid type: `NotAnAttributeType` (the subject is an attribute type no NGSI-LD attribute has)
#[gtest]
#[tokio::test]
async fn n_att_rpl_7200_a_fragment_of_an_unknown_attribute_type_is_bad_request_data_and_replaces_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 5)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    // The model refuses this attribute, so the body is sent as written.
    let request = request_to(&broker, Method::PUT, &format!("entities/{vehicle_id}/attrs/speed"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from("{\n    \"type\": \"NotAnAttributeType\",\n    \"value\": 1\n}"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_id)
        .locality(Locality::Local)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", property().value(5)));
    Ok(())
}
