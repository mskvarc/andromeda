//! An unreachable context outranks an embedded one.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    namespace::Namespace,
    notification_sink::NotificationSink,
    partial_outcome::reported,
    problem::problem,
    report_matcher::batch_error,
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::{
        BatchOperationResult,
        ContextDelivery,
        UncheckedDocument,
        operation::{BatchCreate, CreateEntity},
    },
    model::{Context, ContextEntry, Entity, ErrorType, Lossless, RawJson},
};

/// Pins: Create Entity and Create Subscription under an unreachable @context with an @context embedded below the
/// top level answer 504 `LdContextNotAvailable`, and Batch Entity Creation of such an entity reports
/// `LdContextNotAvailable` against it
/// Clause: 5.5.4, 5.5.6, 5.5.7, 6.3.2
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9025_an_unreachable_context_outranks_an_embedded_one() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let linked = broker.with_context(Context::Single(ContextEntry::Reference(IriRefBuf::new(format!(
        "{unreachable}/ctx/{namespace}/unreachable.jsonld"
    ))?)))?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let body_context = broker
        .with_context(Context::Multiple(vec![ContextEntry::Reference(IriRefBuf::new(format!(
            "{unreachable}/ctx/{namespace}/unreachable.jsonld"
        ))?)]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let device_batched_id = namespace.urn("Device", "batched")?;
    let device = Entity::builder().id(device_id).entity_type(device_type.clone()).build()?;
    // The model refuses it (member `@context` is not allowed here at speed), so `speed` is written as text.
    let speed: RawJson = r#"{"type": "Property", "value": 1, "@context": {}}"#.parse()?;
    let document = UncheckedDocument::from_entity(&device, Lossless::Normalized).set_member("speed", &speed);
    let request = linked.unchecked(&CreateEntity::builder().entity(&device).build())?.document(&document);
    let answer = linked.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::GATEWAY_TIMEOUT, ErrorType::LdContextNotAvailable));
    let endpoint = sink.endpoint(1)?;
    // The model refuses this subscription, so it is sent as written.
    let request_2 = request_to(&linked, Method::POST, "subscriptions")?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n    \"id\": \"urn:ngsi-ld:Subscription:{namespace}-1\",\n    \"type\": \"Subscription\",\n    \"entities\": [\n        {{\n            \"type\": \"Device_{namespace}\",\n            \"@context\": {{}}\n        }}\n    ],\n    \"notification\": {{\n        \"endpoint\": {{\n            \"uri\": \"{endpoint}\",\n            \"accept\": \"application/json\"\n        }}\n    }}\n}}")));
    let answer_2 = linked.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::GATEWAY_TIMEOUT, ErrorType::LdContextNotAvailable));
    let device_batched = Entity::builder().id(device_batched_id.clone()).entity_type(device_type.clone()).build()?;
    // The model refuses it (member `@context` is not allowed here at speed), so `speed` is written as text.
    let speed_2: RawJson = r#"{"type": "Property", "value": 1, "@context": {}}"#.parse()?;
    let document_2 = UncheckedDocument::from_entity(&device_batched, Lossless::Normalized).set_member("speed", &speed_2);
    let entities = [device_batched];
    let request_3 = body_context
        .unchecked(&BatchCreate::builder().entities(&entities).build())?
        .documents(&[document_2]);
    let answer_3 = body_context.send_unchecked(request_3).await;
    let report: BatchOperationResult = reported(answer_3)?;
    expect_that!(
        &report.errors,
        contains(batch_error().entity(&device_batched_id).problem(ErrorType::LdContextNotAvailable))
    );
    Ok(())
}
