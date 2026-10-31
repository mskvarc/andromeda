//! A capitalised context relation names the context and counts.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    config::SuiteConfig,
    entity_matcher::has_attribute,
    namespace::Namespace,
    problem::{answered, problem},
    remote_context::RemoteContexts,
    wire::request_to,
};
use googletest::prelude::*;
use http::{
    HeaderValue,
    Method,
    StatusCode,
    header::{CONTENT_TYPE, LINK},
};
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType},
};

/// Pins: A create under a Link whose context relation is spelled in capitals expands through the @context it names,
/// and a retrieval naming it beside a second context link is refused as `BadRequestData`
/// Clause: 6.3.5, RFC 8288 s2.1.2, JSON-LD 1.1 s6.1
#[gtest]
#[tokio::test]
async fn n_ctx_exp_0033_a_capitalised_context_relation_names_the_context_and_counts() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let core = SuiteConfig::get()?.core_context();
    contexts
        .serve(
            "jc/capitalised.jsonld",
            &format!(r#"{{"@context": {{"speed": "https://test.andromeda/{namespace}/a/speed"}}}}"#),
            Some("application/ld+json"),
        )
        .await?;
    let served = contexts.url("jc/capitalised.jsonld")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 7)
        .build()?;
    // The relation is spelled in capitals, which no typed Link writes: the create is sent as written.
    let request = request_to(&broker, Method::POST, "entities")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .append_header(
            LINK,
            HeaderValue::from_str(&format!(
                r#"<{served}>; rel="HTTP://WWW.W3.ORG/ns/json-ld#context"; type="application/ld+json""#
            ))?,
        )
        .document(&UncheckedDocument::from_entity(&vehicle, Lossless::Normalized));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    expect_that!(
        retrieved.body(),
        has_attribute(&format!("https://test.andromeda/{namespace}/a/speed"), any_attribute().value(7))
    );
    // Two context Link lines on one read are the refusal the test checks.
    let request_2 = request_to(&broker, Method::GET, &format!("entities/{vehicle_id}"))?
        .append_header(
            LINK,
            HeaderValue::from_str(&format!(
                r#"<{served}>; rel="HTTP://WWW.W3.ORG/ns/json-ld#context"; type="application/ld+json""#
            ))?,
        )
        .append_header(
            LINK,
            HeaderValue::from_str(&format!(r#"<{core}>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json""#))?,
        );
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
