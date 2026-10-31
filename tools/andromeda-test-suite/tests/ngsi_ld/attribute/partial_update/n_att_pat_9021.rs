//! An attribute fragment forging a seal is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::model::{Entity, ErrorType, Value};

/// Pins: Partial Attribute Update and Replace Attribute of temperature with a fragment carrying `entityIdSealed`
/// answer 400 `BadRequestData`
/// Clause: 4.5.2.2, 5.6.4.4, 5.6.19.4
#[gtest]
#[tokio::test]
async fn n_att_pat_9021_an_attribute_fragment_forging_a_seal_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_fragment_forged_id = namespace.urn("Building", "fragment-forged")?;
    let building_type = namespace.entity_type("Building")?;
    let building_fragment_forged = Entity::builder()
        .id(building_fragment_forged_id.clone())
        .entity_type(building_type)
        .property("temperature", Value::Number("21.5".parse()?))
        .build()?;
    broker.entities().create(&building_fragment_forged).send().await?.done()?;
    // The model refuses this attribute, so the body is sent as written.
    let request = request_to(&broker, Method::PATCH, &format!("entities/{building_fragment_forged_id}/attrs/temperature"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(format!(
            "{{\n    \"type\": \"Property\",\n    \"value\": 22.5,\n    \"entityIdSealed\": \"urn:ngsi-ld:Building:{namespace}-forged\"\n}}"
        )));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The model refuses this attribute, so the body is sent as written.
    let request_2 = request_to(&broker, Method::PUT, &format!("entities/{building_fragment_forged_id}/attrs/temperature"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(format!(
            "{{\n    \"type\": \"Property\",\n    \"value\": 22.5,\n    \"entityIdSealed\": \"urn:ngsi-ld:Building:{namespace}-forged\"\n}}"
        )));
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
