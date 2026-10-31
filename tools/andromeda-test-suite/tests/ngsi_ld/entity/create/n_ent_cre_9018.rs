//! A create refuses a json literal holding the null under a key.

use andromeda_test_suite::{
    broker,
    core_context::core_entry,
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{Context, ContextEntry, Entity, ErrorType, InlineContext, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Create Entity whose Property value holds a JSON literal (an @json-coerced member or an @json value object)
/// with urn:ngsi-ld:null under a key answers 400 `BadRequestData` and stores nothing
/// Clause: 5.5.4, 5.6.1.4
#[gtest]
#[tokio::test]
async fn n_ent_cre_9018_a_create_refuses_a_json_literal_holding_the_null_under_a_key() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let body_context = broker
        .with_context(Context::Multiple(vec![
            ContextEntry::Inline(r#"{"settings": {"@id": "https://example.org/settings", "@type": "@json"}}"#.parse::<InlineContext>()?),
            core_entry()?,
        ]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let literal_probe_create_keyed_id = namespace.urn("LiteralProbe", "create-keyed")?;
    let literal_probe_type = namespace.entity_type("LiteralProbe")?;
    let literal_probe_create_typed_id = namespace.urn("LiteralProbe", "create-typed")?;
    let literal_probe_create_keyed = Entity::builder()
        .id(literal_probe_create_keyed_id.clone())
        .entity_type(literal_probe_type.clone())
        .build()?;
    // The model refuses it (a request cannot hold the NGSI-LD Null in a member of `value` at config), so `config` is written as text.
    let config_2: RawJson = r#"{"type": "Property", "value": {"settings": {"a": 1, "b": "urn:ngsi-ld:null"}}}"#.parse()?;
    let document = UncheckedDocument::from_entity(&literal_probe_create_keyed, Lossless::Normalized).set_member("config", &config_2);
    let request = body_context
        .unchecked(&CreateEntity::builder().entity(&literal_probe_create_keyed).build())?
        .document(&document);
    let answer = body_context.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&literal_probe_create_keyed_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let literal_probe_create_typed = Entity::builder()
        .id(literal_probe_create_typed_id.clone())
        .entity_type(literal_probe_type.clone())
        .build()?;
    // The model refuses it (a request cannot hold the NGSI-LD Null in a member of `value` at config), so `config` is written as text.
    let config_3: RawJson = r#"{"type": "Property", "value": {"@type": "@json", "@value": {"a": 1, "b": "urn:ngsi-ld:null"}}}"#.parse()?;
    let document_2 = UncheckedDocument::from_entity(&literal_probe_create_typed, Lossless::Normalized).set_member("config", &config_3);
    let request_2 = body_context
        .unchecked(&CreateEntity::builder().entity(&literal_probe_create_typed).build())?
        .document(&document_2);
    let answer_3 = body_context.send_unchecked(request_2).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&literal_probe_create_typed_id)
        .send()
        .await;
    expect_that!(&answer_4, answered(StatusCode::NOT_FOUND));
    Ok(())
}
