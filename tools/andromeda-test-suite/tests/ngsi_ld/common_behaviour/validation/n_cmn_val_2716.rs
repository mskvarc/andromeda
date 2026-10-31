//! Replace attribute with a fragment id naming another entity is bad request data.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
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
    model::{Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: a Replace Attribute whose Fragment holds an id naming another Entity is refused with 400 `BadRequestData`,
/// and the target is unchanged
/// Clause: 5.4, 5.5.4, 5.6.19.4, 6.7.3.3
#[gtest]
#[tokio::test]
async fn n_cmn_val_2716_replace_attribute_with_a_fragment_id_naming_another_entity_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 55)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    // The model refuses this attribute, so the body is sent as written.
    let request = request_to(&broker, Method::PUT, &format!("entities/{vehicle_id}/attrs/speed"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(format!(
            "{{\n    \"id\": \"urn:ngsi-ld:Vehicle:{namespace}-2\",\n    \"type\": \"Property\",\n    \"value\": 99\n}}"
        )));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(55)));
    Ok(())
}
