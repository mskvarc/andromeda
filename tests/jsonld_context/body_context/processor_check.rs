//! A rendered body reads as NGSI-LD under nothing but the `@context` it names
//! (clause 4.4; study A2 to A5 and A7, "processor check").
//!
//! Each case retrieves an Entity as `application/ld+json`, hands the body to an
//! independent JSON-LD 1.1 processor that resolves only the documents the body
//! names, and compares what it reads with what the broker stored: the same type
//! IRI and the same attribute IRIs.

use crate::{
    broker_endpoints::broker_url,
    context_links::context_link,
    core_contexts::{CORE_V18, core},
    independent_processor::IndependentProcessor,
    ngsi_requests::{get, ok_body, stored_entity},
    private_tenant::PrivateTenant,
    unique_ids::unique_id,
    user_contexts::{DEVICE_IRI, UserContexts, term_definitions},
};
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use std::collections::BTreeSet;

/// Create a `Device` under the one context `link` names, carrying `speed`,
/// which every user document defines, and `colour`, which none does, so it is
/// read through the core `@context`'s `@vocab`.
async fn create_device_with_an_undefined_term(client: &Client, link: &str) -> String {
    let id = unique_id("urn:ngsi-ld:Device:processor-check");
    let response = client
        .post(broker_url("entities"))
        .header("Content-Type", "application/json")
        .header("Link", context_link(link))
        .json(&json!({
            "id": id,
            "type": "Device",
            "speed": { "type": "Property", "value": 3 },
            "colour": { "type": "Property", "value": "red" }
        }))
        .send()
        .await
        .expect("create request failed");
    assert_eq!(response.status(), StatusCode::CREATED, "{:?}", response.text().await);
    id
}

/// The documents the rendered `@context` names, fetched as a remote processor
/// would.
async fn processor_for(rendered: &Value) -> IndependentProcessor {
    let mut processor = IndependentProcessor::new();
    let named: Vec<&str> = match &rendered["@context"] {
        Value::String(reference) => vec![reference.as_str()],
        Value::Array(entries) => entries.iter().filter_map(Value::as_str).collect(),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_) => Vec::new(),
    };
    for reference in named {
        if reference != core() {
            processor = processor.fetch(reference).await;
        }
    }
    processor
}

/// Assert the independent processor reads `rendered` as the Entity `id` the
/// broker stored.
async fn assert_reads_as_stored(client: &Client, id: &str, rendered: &Value) {
    let processor = processor_for(rendered).await;
    let read = processor.read(rendered).await;
    assert_eq!(read.len(), 1, "one node: {rendered}");
    let node = &read[0];
    assert_eq!(node.id.as_deref(), Some(id));
    assert_eq!(node.types, BTreeSet::from([DEVICE_IRI.to_owned()]), "{rendered}");
    let stored = stored_entity(client, id).await;
    let stored_node = processor.read_stored(&stored).await;
    assert_eq!(node.types, stored_node.types, "rendered {rendered}\nstored {stored}");
    assert_eq!(node.properties, stored_node.properties, "rendered {rendered}\nstored {stored}");
}

/// Study A2, A3 and A4 with the processor check, for a request context named
/// in a `Link`: a context ending with the core `@context`, one without it, and
/// one setting its own `@vocab` after importing it.
#[tokio::test]
async fn a_body_rendered_under_a_linked_context_reads_as_the_stored_entity() {
    let tenant = PrivateTenant::provisioned().await;
    let contexts = UserContexts::start().await;
    let id = create_device_with_an_undefined_term(tenant.client(), &contexts.terms).await;

    for linked in [&contexts.wrapper, &contexts.terms, &contexts.vocab_after_core, &contexts.importing] {
        let rendered = ok_body(get(tenant.client(), &format!("entities/{id}"), "application/ld+json", Some(linked)).await).await;
        assert_reads_as_stored(tenant.client(), &id, &rendered).await;
    }
    contexts.shutdown().await;
    tenant.finish().await;
}

/// Study A5 and A7 with the processor check, for a request context named in
/// an `application/ld+json` POST query body: an inline definition, and an older
/// core release before a user context.
#[tokio::test]
async fn a_body_rendered_under_a_body_context_reads_as_the_stored_entity() {
    let tenant = PrivateTenant::provisioned().await;
    let contexts = UserContexts::start().await;
    let id = create_device_with_an_undefined_term(tenant.client(), &contexts.terms).await;

    for context in [term_definitions(), json!([CORE_V18, contexts.terms])] {
        let response = tenant
            .client()
            .post(broker_url("entityOperations/query"))
            .header("Content-Type", "application/ld+json")
            .header("Accept", "application/ld+json")
            .json(&json!({ "type": "Query", "entities": [{ "type": DEVICE_IRI, "id": id }], "@context": context }))
            .send()
            .await
            .expect("POST query failed");
        let answered = ok_body(response).await;
        let rendered = &answered.as_array().expect("an array")[0];
        assert_reads_as_stored(tenant.client(), &id, rendered).await;
    }
    contexts.shutdown().await;
    tenant.finish().await;
}

/// Clause 4.4 read back by the broker itself: a body rendered under a request
/// context and written back as `application/ld+json`, under nothing but the
/// `@context` it names, is stored with the type and attribute IRIs of the
/// original.
#[tokio::test]
async fn a_rendered_body_written_back_under_its_own_context_stores_the_same_iris() {
    let tenant = PrivateTenant::provisioned().await;
    let contexts = UserContexts::start().await;
    let id = create_device_with_an_undefined_term(tenant.client(), &contexts.terms).await;
    let mut original = stored_entity(tenant.client(), &id).await;
    original.as_object_mut().expect("an entity object").remove("id");
    let original = original;

    for linked in [&contexts.wrapper, &contexts.terms, &contexts.vocab_after_core, &contexts.importing] {
        let mut rendered = ok_body(get(tenant.client(), &format!("entities/{id}"), "application/ld+json", Some(linked)).await).await;
        let copy = unique_id("urn:ngsi-ld:Device:written-back");
        rendered["id"] = json!(copy);
        let written = tenant
            .client()
            .post(broker_url("entities"))
            .header("Content-Type", "application/ld+json")
            .json(&rendered)
            .send()
            .await
            .expect("write-back request failed");
        assert_eq!(written.status(), StatusCode::CREATED, "{linked}: {:?}", written.text().await);

        let mut copied = stored_entity(tenant.client(), &copy).await;
        copied.as_object_mut().expect("an entity object").remove("id");
        assert_eq!(copied, original, "{linked}: the copy stores what the original does");
    }
    contexts.shutdown().await;
    tenant.finish().await;
}
