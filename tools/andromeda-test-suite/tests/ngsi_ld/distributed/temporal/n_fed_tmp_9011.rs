//! A temporal create forwards every scope instance.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    namespace::Namespace,
    problem::answered,
    source_answer,
    source_request::SourceRequest,
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    query::IdPattern,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};
use serde_json::Value;
use std::str;

/// Pins: Create Temporal Evolution of an Entity whose scope holds an instance set on 2025-01-01 and its removal on
/// 2025-01-02, matched by an inclusive registration for `upsertTemporal`, forwards both scope instances with their
/// `observedAt` and `deletedAt` and the speed instances, and none of the `createdAt` and `modifiedAt` the client sent
/// Clause: 4.5.6, 4.18, 5.6.11.4, 5.2.5
#[gtest]
#[tokio::test]
async fn n_fed_tmp_9011_a_temporal_create_forwards_every_scope_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let reified_scope_forwarding_type = namespace.entity_type("ReifiedScopeForwarding")?;
    let reified_scope_forwarding_id = namespace.urn("ReifiedScopeForwarding", "1")?;
    let registration_id = namespace.registration_id(1)?;
    source
        .answer(
            Method::POST,
            "/scope/ngsi-ld/v1/temporal/entities",
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("scope")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(reified_scope_forwarding_type).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::UpsertTemporal])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // The scope instances carry createdAt, modifiedAt and deletedAt, which no typed temporal fragment writes, so the body is sent as written.
    let request = request_to(&broker, Method::POST, "temporal/entities")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(format!(
            r#"{{"id":"{reified_scope_forwarding_id}","type":"ReifiedScopeForwarding_{namespace}","scope":[{{"type":"Property","value":"/Madrid/Gardens","observedAt":"2025-01-01T00:00:00Z","createdAt":"2025-01-01T00:00:01Z","modifiedAt":"2025-01-01T00:00:02Z"}},{{"type":"Property","value":"urn:ngsi-ld:null","observedAt":"2025-01-02T00:00:00Z","deletedAt":"2025-01-02T00:00:00Z"}}],"speed":[{{"type":"Property","value":10,"observedAt":"2025-01-01T00:00:00Z"}}]}}"#
        )));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let forwarded_request = SourceRequest::new(Method::POST, "/scope/ngsi-ld/v1/temporal/entities");
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_body = forwarded.first().or_fail()?.body.as_slice();
    let document: Value = serde_json::from_slice(forwarded_body)?;
    expect_that!(document.pointer("/scope").and_then(Value::as_array).map(Vec::len), some(eq(2)));
    expect_that!(document.pointer("/scope/0/value").and_then(Value::as_str), some(eq("/Madrid/Gardens")));
    expect_that!(
        document.pointer("/scope/0/observedAt").and_then(Value::as_str),
        some(eq("2025-01-01T00:00:00Z"))
    );
    expect_that!(document.pointer("/scope/1/value").and_then(Value::as_str), some(eq("urn:ngsi-ld:null")));
    expect_that!(document.pointer("/scope/1/deletedAt").and_then(Value::as_str), some(eq("2025-01-02T00:00:00Z")));
    expect_that!(document.pointer("/speed"), some(anything()));
    let forwarded_text = str::from_utf8(forwarded_body)?;
    expect_that!(forwarded_text, not(contains_substring("2025-01-01T00:00:01Z")));
    expect_that!(forwarded_text, not(contains_substring("2025-01-01T00:00:02Z")));
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
