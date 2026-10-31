//! Every batch write under an unretrievable context link fails the whole request.

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
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, ContextEntry, Entity, EntityFragment, ErrorType, NgsiLdMediaType},
};

/// Pins: Batch Entity Create, Upsert, Update and Merge of application/json entities under a context Link that
/// cannot be retrieved each answer 504 `LdContextNotAvailable` problem details rather than 207, an item refused
/// before its context is read included, and write nothing
/// Clause: 5.5.6, 6.3.2, 6.3.5, 5.6.7.4, 5.6.8.4, 5.6.9.4, 5.6.20.4
/// Recorded: 5.5.6 raises `LdContextNotAvailable` for a remote @context referenced by the request and the batch
/// clauses report entity failures per entity; Andromeda treats the Link context, which holds for every entity, as
/// the request's and refuses it whole
#[gtest]
#[tokio::test]
async fn o_ctx_exp_9052_every_batch_write_under_an_unretrievable_context_link_fails_the_whole_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = format!(
        "{}/ctx/{namespace}/unreachable.jsonld",
        config.unreachable_origin().origin().ascii_serialization()
    );
    let linked = broker.with_context(Context::Single(ContextEntry::Reference(IriRefBuf::new(unreachable.clone())?)))?;
    let device_type = namespace.entity_type("Device")?;
    let existing_id = namespace.urn("Device", "existing")?;
    let existing = Entity::builder()
        .id(existing_id.clone())
        .entity_type(device_type.clone())
        .property("speed", 1)
        .build()?;
    broker.entities().create(&existing).send().await?.done()?;
    // An item that carries its own @context beside the others cannot be written by the model, so the create is
    // sent as written.
    let core = config.core_context();
    let body = format!(
        r#"[{{"id": "urn:ngsi-ld:Device:{namespace}-created", "type": "Device_{namespace}", "speed": {{"type": "Property", "value": 2}}}}, {{"id": "urn:ngsi-ld:Device:{namespace}-carrying", "type": "Device_{namespace}", "@context": "{core}"}}]"#
    );
    let create = request_to(&broker, Method::POST, "entityOperations/create")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .append_header(
            LINK,
            HeaderValue::from_str(&format!(
                r#"<{unreachable}>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json""#
            ))?,
        )
        .body(Bytes::from(body));
    let answer = broker.send_unchecked(create).await;
    expect_that!(
        &answer,
        problem(StatusCode::GATEWAY_TIMEOUT, ErrorType::LdContextNotAvailable)
            .with_header_containing(CONTENT_TYPE, "application/json")
            .with_title()
    );
    let upserted_id = namespace.urn("Device", "upserted")?;
    let upserted = Entity::builder()
        .id(upserted_id.clone())
        .entity_type(device_type.clone())
        .property("speed", 2)
        .build()?;
    let existing_upsert = Entity::builder()
        .id(existing_id.clone())
        .entity_type(device_type.clone())
        .property("speed", 2)
        .build()?;
    let answer_2 = linked.with_responses().entities().batch_upsert(&[upserted, existing_upsert]).send().await;
    expect_that!(&answer_2, problem(StatusCode::GATEWAY_TIMEOUT, ErrorType::LdContextNotAvailable));
    let update = EntityFragment::for_entity(existing_id.clone(), device_type.clone())
        .property("speed", 3)
        .build()?;
    let answer_3 = linked.with_responses().entities().batch_update(&[update]).send().await;
    expect_that!(&answer_3, problem(StatusCode::GATEWAY_TIMEOUT, ErrorType::LdContextNotAvailable));
    let merge = EntityFragment::for_entity(existing_id.clone(), device_type.clone())
        .property("speed", 4)
        .build()?;
    let answer_4 = linked.with_responses().entities().batch_merge(&[merge]).send().await;
    expect_that!(&answer_4, problem(StatusCode::GATEWAY_TIMEOUT, ErrorType::LdContextNotAvailable));
    for absent in ["created", "carrying", "upserted"] {
        let answer_5 = broker
            .with_responses()
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(&namespace.urn("Device", absent)?)
            .send()
            .await;
        expect_that!(&answer_5, answered(StatusCode::NOT_FOUND));
    }
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&existing_id)
        .send()
        .await?;
    expect_that!(retrieved.body(), has_attribute("speed", any_attribute().value(1)));
    Ok(())
}
