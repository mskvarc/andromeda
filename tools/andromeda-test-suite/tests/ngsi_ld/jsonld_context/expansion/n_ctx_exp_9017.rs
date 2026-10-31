//! Attribute fragment writes refuse an embedded context.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    config::SuiteConfig,
    core_context::core_context_list,
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
    http::{ContextDelivery, headers::Accept},
    model::{Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: Partial Attribute Update and Replace Attribute whose fragment carries an @context inside a sub-Attribute
/// answer 400 `BadRequestData`, and the attribute keeps its value
/// Clause: 5.5.7, 5.6.4.4, 5.6.19.4
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9017_attribute_fragment_writes_refuse_an_embedded_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let core = SuiteConfig::get()?.core_context();
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let body_context = broker.with_context(core_context_list()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let device = Entity::builder().id(device_id.clone()).entity_type(device_type).property("speed", 1).build()?;
    body_context.entities().create(&device).send().await?.done()?;
    // The model has no sub-Attribute carrying an @context, so both fragments are sent as written.
    let patch_body = r#"{
    "value": 2,
    "accuracy": {
        "type": "Property",
        "value": 0.5,
        "@context": {
            "accuracy": "https://example.org/{{ns}}/accuracy"
        }
    },
    "@context": [
        "{{core}}"
    ]
}"#
    .replace("{{ns}}", &namespace.to_string())
    .replace("{{core}}", core.as_str());
    let request = request_to(&broker, Method::PATCH, &format!("entities/{device_id}/attrs/speed"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
        .body(Bytes::from(patch_body));
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("@context")
    );
    let put_body = r#"{
    "type": "Property",
    "value": 3,
    "accuracy": {
        "type": "Property",
        "value": 0.5,
        "@context": {}
    },
    "@context": [
        "{{core}}"
    ]
}"#
    .replace("{{core}}", core.as_str());
    let request_2 = request_to(&broker, Method::PUT, &format!("entities/{device_id}/attrs/speed"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
        .body(Bytes::from(put_body));
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(
        &answer_2,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("@context")
    );
    let kept = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    expect_that!(kept.body(), has_attribute("speed", any_attribute().value(1)));
    Ok(())
}
