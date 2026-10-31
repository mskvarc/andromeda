//! One link line carrying two context values is refused.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    config::SuiteConfig,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::{answered, problem},
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{
    HeaderValue,
    Method,
    StatusCode,
    header::{CONTENT_TYPE, LINK},
};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: Retrieve Entity, Delete Entity, Create Entity and Update Attributes carrying one Link line with two @context
/// link-values are refused as `BadRequestData`
/// Clause: 6.3.5, RFC 8288 s3, JSON-LD 1.1 s6.1
#[gtest]
#[tokio::test]
async fn n_ctx_exp_0044_one_link_line_carrying_two_context_values_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let core = SuiteConfig::get()?.core_context();
    let two_values = format!(
        r#"<{core}>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json", <https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json""#
    );
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 1)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    // One Link line with two context values is the refusal the test checks, so every request goes as written.
    let request = request_to(&broker, Method::GET, &format!("entities/{vehicle_id}"))?.append_header(LINK, HeaderValue::from_str(&two_values)?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let request_2 = request_to(&broker, Method::DELETE, &format!("entities/{vehicle_id}"))?.append_header(LINK, HeaderValue::from_str(&two_values)?);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let refused_id = namespace.urn("Vehicle", "refused")?;
    let body = format!(
        r#"{{
    "id": "{refused_id}",
    "type": "{}",
    "@context": "{core}"
}}"#,
        namespace.entity_type("Vehicle")?
    );
    let request_3 = request_to(&broker, Method::POST, "entities")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .append_header(LINK, HeaderValue::from_str(&two_values)?)
        .body(Bytes::from(body));
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let patch_body = r#"{
    "speed": {
        "type": "Property",
        "value": 2
    }
}"#;
    let request_4 = request_to(&broker, Method::PATCH, &format!("entities/{vehicle_id}/attrs"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .append_header(LINK, HeaderValue::from_str(&two_values)?)
        .body(Bytes::from(patch_body));
    let answer_4 = broker.send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let kept = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    expect_that!(kept.body(), has_attribute("speed", any_attribute().value(1)));
    let refused = broker.with_responses().entities().retrieve(&refused_id).send().await;
    expect_that!(&refused, answered(StatusCode::NOT_FOUND));
    Ok(())
}
