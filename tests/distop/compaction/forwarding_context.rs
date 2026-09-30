//! The one `@context` a forwarded request is expressed against (clauses 4.3.6.6,
//! 6.3.5 and 6.3.19).
//!
//! For every registration and every request forwarded as `application/json`
//! the broker chooses exactly one forwarding context: the registration's
//! `jsonldContext`, else the inbound context when it is a single absolute URL,
//! else the core context. The request carries at most one context `Link`, to
//! that context, and every name in its query and body is compacted against it;
//! the source's answer is expanded against it and read back in the client's
//! terms. A request forwarded as `application/ld+json` carries the inbound
//! context inline and no context `Link`. No forwarded request ever carries a
//! relative context reference or a second context link-value (JSON-LD 1.1
//! section 6.1: a single context link).
//!
//! Two hosted documents give the same two IRIs different spellings: the
//! client's (`spd`, `Veh`) and a registration's configured one (`velocity`,
//! `Car`). Which spelling reaches the mock source proves which context compacted
//! the forwarded request.

use crate::{
    compaction::{CAPTURE_TIMEOUT, CORE_CONTEXT, FORWARDED_ENTITIES, SPEED_IRI, VEHICLE_IRI, aliased_document, registration_context, serve},
    context_source_adapter::ContextSourceAdapter,
    hosted_context::{context_link, forget_cached, start_origin},
    oracle::{assert_status, forwarded_header, location},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use andromeda_test_mocks::callback_server::CapturedRequest;
use axum::http::Method;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use url::{Url, form_urlencoded};

/// The path a broker appends when forwarding a batch entity query (clause 6.23).
const FORWARDED_BATCH_QUERY: &str = "/ngsi-ld/v1/entityOperations/query";

/// The path a broker appends when forwarding a batch entity creation (clause
/// 6.14).
const FORWARDED_BATCH_CREATE: &str = "/ngsi-ld/v1/entityOperations/create";

/// The link relation that marks a `Link` link-value as a JSON-LD context
/// (JSON-LD 1.1 section 6.1).
const CONTEXT_RELATION: &str = "http://www.w3.org/ns/json-ld#context";

/// The IRI of an attribute whose client term is `attrs`, the name of the
/// attribute collection in a resource path.
const ATTRS_IRI: &str = "https://example.org/vocab/attrs";

/// A registration's configured `jsonldContext`: the client's IRIs under other
/// terms, so a forwarded `velocity`/`Car` can only come from this document.
fn configured_document() -> String {
    json!({ "@context": { "velocity": SPEED_IRI, "Car": VEHICLE_IRI } }).to_string()
}

/// A second client document naming a term the query does not use, so a request
/// listing it beside the client's document carries a compound `@context`.
fn brand_document() -> String {
    json!({ "@context": { "brand": "https://example.org/vocab/brand" } }).to_string()
}

/// A client document spelling the speed IRI `rate`, so two batch items can name
/// the same attribute through two different contexts.
fn rate_document() -> String {
    json!({ "@context": { "rate": SPEED_IRI, "Veh": VEHICLE_IRI } }).to_string()
}

/// The targets of every context link-value the forwarded request carries, over
/// every `Link` header line, in order.
fn context_link_targets(request: &CapturedRequest) -> Vec<String> {
    request
        .headers
        .get_all("link")
        .iter()
        .filter_map(|line| line.to_str().ok())
        .flat_map(|line| line.split(','))
        .filter(|link_value| link_value.contains(CONTEXT_RELATION))
        .filter_map(|link_value| link_value.split_once('<').and_then(|(_, rest)| rest.split_once('>')))
        .map(|(target, _)| target.trim().to_owned())
        .collect()
}

/// Assert the forwarded request names at most one context, and that every
/// context it names is an absolute URL (clause 6.3.5; JSON-LD 1.1 section 6.1).
fn assert_at_most_one_absolute_context_link(request: &CapturedRequest) -> Option<String> {
    let targets = context_link_targets(request);
    assert!(targets.len() <= 1, "a forwarded request carries at most one context link-value: {targets:?}");
    for target in &targets {
        let parsed = Url::parse(target);
        assert!(parsed.is_ok(), "a forwarded context reference is absolute: {target}");
        let parsed = parsed.expect("an absolute URL");
        assert!(!parsed.cannot_be_a_base(), "a forwarded context reference is a hierarchical URL: {target}");
    }
    targets.into_iter().next()
}

/// Assert the forwarded request names exactly `expected` as its one context.
fn assert_single_context_link(request: &CapturedRequest, expected: &str) {
    assert_eq!(
        assert_at_most_one_absolute_context_link(request).as_deref(),
        Some(expected),
        "the forwarded request carries exactly one context link-value, naming the forwarding context"
    );
}

/// Assert the forwarded request names no context other than the core, whose
/// absence means the same thing (clause 6.3.5).
fn assert_core_or_no_context_link(request: &CapturedRequest) {
    if let Some(target) = assert_at_most_one_absolute_context_link(request) {
        assert_eq!(target, CORE_CONTEXT, "a compound inbound context is forwarded against the core context");
    }
}

/// The decoded value of one query parameter of the forwarded request.
fn forwarded_parameter(request: &CapturedRequest, name: &str) -> Option<String> {
    form_urlencoded::parse(request.uri.query().unwrap_or_default().as_bytes())
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
}

/// The first entity of a query answer, whichever of an array or a single
/// entity the broker answered with.
fn first_entity(answer: &Value) -> &Value {
    answer.as_array().and_then(|entities| entities.first()).unwrap_or(answer)
}

/// H1: a forwarded GET for a request naming one user context in its `Link`
/// carries exactly one context link-value, naming that URL, and its `type`,
/// `attrs` and `q` names are the request's own terms (clause 6.3.5).
#[serial_test::serial(distop_shared_type)]
#[tokio::test]
async fn distop_forwarding_context_a_single_inbound_link_is_forwarded_as_the_one_context() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &json!([])).await;

    let origin = start_origin().await;
    let client_context = serve(&origin, &aliased_document()).await;

    let registration = CsrBuilder::new(&topology.id("registration:forwarding-context-h1"), &mock.url(""))
        .mode("inclusive")
        .entity_type("Veh")
        .property_names(&["spd"])
        .operations(&["queryEntity"])
        .context(registration_context(&client_context))
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("create registration"), 201);

    let response = front
        .client()
        .query_entities("type=Veh&attrs=spd&q=spd%3E1", "application/ld+json", Some(&client_context))
        .await
        .expect("query");
    assert_status(&response, 200);

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    assert_single_context_link(&forwarded, &client_context);
    assert_eq!(forwarded_parameter(&forwarded, "type").as_deref(), Some("Veh"));
    assert_eq!(forwarded_parameter(&forwarded, "attrs").as_deref(), Some("spd"));
    assert_eq!(forwarded_parameter(&forwarded, "q").as_deref(), Some("spd>1"));

    forget_cached(front.pool(), &[client_context]).await;
    mock.shutdown().await;
    origin.shutdown().await.expect("context server shuts down");
    topology.finish().await.expect("topology cleanup");
}

/// H2: an `application/ld+json` POST query whose `@context` lists two
/// documents is forwarded, to a `queryBatch` source, against the core context:
/// at most one context link-value (the core), fully qualified names in the
/// forwarded `Query` body and no inline `@context`, while the source's fully
/// qualified answer reaches the client in the client's own terms (clauses
/// 4.3.6.6, 6.3.5, 6.23).
#[serial_test::serial(distop_shared_type)]
#[tokio::test]
async fn distop_forwarding_context_a_compound_body_context_is_forwarded_against_the_core() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;
    let remote = topology.id("Veh:forwarding-context-h2");
    mock.set_stub_with_body(
        Method::POST,
        FORWARDED_BATCH_QUERY,
        200,
        &json!([{ "id": remote, "type": VEHICLE_IRI, SPEED_IRI: { "type": "Property", "value": 5 } }]),
    )
    .await;

    let origin = start_origin().await;
    let client_context = serve(&origin, &aliased_document()).await;
    let brand_context = serve(&origin, &brand_document()).await;

    let registration = CsrBuilder::new(&topology.id("registration:forwarding-context-h2"), &mock.url(""))
        .mode("inclusive")
        .entity_type("Veh")
        .property_names(&["spd"])
        .operations(&["queryBatch"])
        .context(registration_context(&client_context))
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("create registration"), 201);

    let query = json!({
        "type": "Query",
        "entities": [{ "type": "Veh" }],
        "attrs": ["spd"],
        "@context": [client_context, brand_context]
    });
    let response = front
        .client()
        .raw(Method::POST, FORWARDED_BATCH_QUERY)
        .header("Content-Type", "application/ld+json")
        .header("Accept", "application/ld+json")
        .json(&query)
        .send()
        .await
        .expect("batch query");
    assert_status(&response, 200);

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    assert_eq!(forwarded.uri.path(), FORWARDED_BATCH_QUERY);
    assert_core_or_no_context_link(&forwarded);
    let body: Value = forwarded.json().expect("the forwarded Query is JSON");
    assert!(body.get("@context").is_none(), "an application/json body carries no inline @context: {body}");
    assert_eq!(body["entities"][0]["type"], json!(VEHICLE_IRI), "the type is fully qualified: {body}");
    assert_eq!(body["attrs"], json!([SPEED_IRI]), "the attribute is fully qualified: {body}");

    let answer = response.json_value().expect("the merged answer is JSON");
    let entity = first_entity(&answer);
    assert_eq!(entity["type"], json!("Veh"), "the answer is compacted with the client's context: {answer}");
    assert_eq!(
        entity["spd"]["value"],
        json!(5),
        "the answer names the attribute in the client's terms: {answer}"
    );

    forget_cached(front.pool(), &[client_context, brand_context]).await;
    mock.shutdown().await;
    origin.shutdown().await.expect("context server shuts down");
    topology.finish().await.expect("topology cleanup");
}

/// H2: an `application/ld+json` POST query with an inline `@context` object is
/// forwarded, to a `queryEntity` source, as a GET whose query names are fully
/// qualified and whose only possible context link-value is the core (clauses
/// 4.3.6.6, 6.3.5).
#[serial_test::serial(distop_shared_type)]
#[tokio::test]
async fn distop_forwarding_context_an_inline_body_context_is_forwarded_against_the_core() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;
    let remote = topology.id("Veh:forwarding-context-h2-inline");
    mock.set_stub_with_body(
        Method::GET,
        FORWARDED_ENTITIES,
        200,
        &json!([{ "id": remote, "type": VEHICLE_IRI, SPEED_IRI: { "type": "Property", "value": 7 } }]),
    )
    .await;

    let origin = start_origin().await;
    let client_context = serve(&origin, &aliased_document()).await;

    let registration = CsrBuilder::new(&topology.id("registration:forwarding-context-h2-inline"), &mock.url(""))
        .mode("inclusive")
        .entity_type("Veh")
        .property_names(&["spd"])
        .operations(&["queryEntity"])
        .context(registration_context(&client_context))
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("create registration"), 201);

    let query = json!({
        "type": "Query",
        "entities": [{ "type": "Veh" }],
        "attrs": ["spd"],
        "@context": { "spd": SPEED_IRI, "Veh": VEHICLE_IRI }
    });
    let response = front
        .client()
        .raw(Method::POST, FORWARDED_BATCH_QUERY)
        .header("Content-Type", "application/ld+json")
        .header("Accept", "application/ld+json")
        .json(&query)
        .send()
        .await
        .expect("batch query");
    assert_status(&response, 200);

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    assert_eq!(forwarded.uri.path(), FORWARDED_ENTITIES);
    assert_core_or_no_context_link(&forwarded);
    assert_eq!(forwarded_parameter(&forwarded, "type").as_deref(), Some(VEHICLE_IRI));
    assert_eq!(forwarded_parameter(&forwarded, "attrs").as_deref(), Some(SPEED_IRI));

    let answer = response.json_value().expect("the merged answer is JSON");
    let entity = first_entity(&answer);
    assert_eq!(entity["type"], json!("Veh"), "the answer is compacted with the client's context: {answer}");
    assert_eq!(
        entity["spd"]["value"],
        json!(7),
        "the answer names the attribute in the client's terms: {answer}"
    );

    forget_cached(front.pool(), &[client_context]).await;
    mock.shutdown().await;
    origin.shutdown().await.expect("context server shuts down");
    topology.finish().await.expect("topology cleanup");
}

/// H3: a registration whose `contextSourceInfo` names a `jsonldContext` X is
/// sent one context link-value, to X, and every name of the forwarded query is
/// compacted against X rather than the client's context; the source's answer,
/// spelled in X, reaches the client in the client's own terms (clauses 4.3.6.6,
/// 6.3.19).
#[serial_test::serial(distop_shared_type)]
#[tokio::test]
async fn distop_forwarding_context_a_configured_jsonld_context_spells_the_forwarded_query() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;
    let remote = topology.id("Veh:forwarding-context-h3");
    mock.set_stub_with_body(
        Method::GET,
        FORWARDED_ENTITIES,
        200,
        &json!([{ "id": remote, "type": "Car", "velocity": { "type": "Property", "value": 3 } }]),
    )
    .await;

    let origin = start_origin().await;
    let client_context = serve(&origin, &aliased_document()).await;
    let configured_context = serve(&origin, &configured_document()).await;

    let registration = CsrBuilder::new(&topology.id("registration:forwarding-context-h3"), &mock.url(""))
        .mode("inclusive")
        .entity_type("Veh")
        .property_names(&["spd"])
        .operations(&["queryEntity"])
        .context_source_info("jsonldContext", &configured_context)
        .context(registration_context(&client_context))
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("create registration"), 201);

    let response = front
        .client()
        .query_entities("type=Veh&attrs=spd&q=spd%3E1", "application/ld+json", Some(&client_context))
        .await
        .expect("query");
    assert_status(&response, 200);

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    assert_single_context_link(&forwarded, &configured_context);
    assert_eq!(forwarded_parameter(&forwarded, "type").as_deref(), Some("Car"));
    assert_eq!(forwarded_parameter(&forwarded, "attrs").as_deref(), Some("velocity"));
    assert_eq!(forwarded_parameter(&forwarded, "q").as_deref(), Some("velocity>1"));
    let query = forwarded.uri.query().unwrap_or_default();
    assert!(
        !query.contains("spd") && !query.contains("Veh"),
        "no client spelling reaches the source: {query}"
    );

    let answer = response.json_value().expect("the merged answer is JSON");
    let entity = first_entity(&answer);
    assert_eq!(entity["type"], json!("Veh"), "the answer is read back in the client's terms: {answer}");
    assert_eq!(entity["spd"]["value"], json!(3), "the answer is read back in the client's terms: {answer}");
    assert!(entity.get("velocity").is_none(), "the source's spelling does not leak: {answer}");

    forget_cached(front.pool(), &[client_context, configured_context]).await;
    mock.shutdown().await;
    origin.shutdown().await.expect("context server shuts down");
    topology.finish().await.expect("topology cleanup");
}

/// H3: a creation forwarded to a registration with a configured `jsonldContext`
/// X carries one context link-value, to X, and its body is compacted against X
/// with no inline `@context` (clause 4.3.6.6: "over both payload and query
/// parameters").
#[serial_test::serial(distop_shared_type)]
#[tokio::test]
async fn distop_forwarding_context_a_configured_jsonld_context_spells_the_forwarded_body() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub_with_body(Method::POST, FORWARDED_ENTITIES, 201, &json!({})).await;

    let origin = start_origin().await;
    let client_context = serve(&origin, &aliased_document()).await;
    let configured_context = serve(&origin, &configured_document()).await;

    let entity_id = topology.id("Veh:forwarding-context-h3-body");
    let registration = CsrBuilder::new(&topology.id("registration:forwarding-context-h3-body"), &mock.url(""))
        .mode("inclusive")
        .entity_id(&entity_id)
        .entity_type("Veh")
        .property_names(&["spd"])
        .operations(&["createEntity"])
        .context_source_info("jsonldContext", &configured_context)
        .context(registration_context(&client_context))
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("create registration"), 201);

    let entity = json!({ "id": entity_id, "type": "Veh", "spd": { "type": "Property", "value": 30 } });
    let created = front
        .client()
        .raw(Method::POST, FORWARDED_ENTITIES)
        .header("Content-Type", "application/json")
        .header("Link", &context_link(&client_context))
        .json(&entity)
        .send()
        .await
        .expect("create entity");
    assert!(created.status.is_success(), "the creation succeeds: {}", created.status);

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    assert_single_context_link(&forwarded, &configured_context);
    let content_type = forwarded_header(&forwarded, "content-type").unwrap_or_default();
    assert!(
        content_type.starts_with("application/json"),
        "a configured context forwards JSON: {content_type}"
    );
    let body: Value = forwarded.json().expect("the forwarded entity is JSON");
    assert!(body.get("@context").is_none(), "the JSON body carries no inline @context: {body}");
    assert_eq!(body["type"], json!("Car"), "the type is spelled in the configured context: {body}");
    assert_eq!(
        body["velocity"]["value"],
        json!(30),
        "the attribute is spelled in the configured context: {body}"
    );
    assert!(body.get("spd").is_none(), "no client spelling reaches the source: {body}");

    forget_cached(front.pool(), &[client_context, configured_context]).await;
    mock.shutdown().await;
    origin.shutdown().await.expect("context server shuts down");
    topology.finish().await.expect("topology cleanup");
}

/// H4: a registration whose `contextSourceInfo` names the `contentType`
/// `application/ld+json` is sent no context link-value; the body carries the
/// inbound context inline, as that MIME type requires (clauses 4.3.6.6, 6.3.5).
#[serial_test::serial(distop_shared_type)]
#[tokio::test]
async fn distop_forwarding_context_an_ld_json_source_receives_the_inbound_context_inline() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub_with_body(Method::POST, FORWARDED_ENTITIES, 201, &json!({})).await;

    let origin = start_origin().await;
    let client_context = serve(&origin, &aliased_document()).await;

    let entity_id = topology.id("Veh:forwarding-context-h4");
    let registration = CsrBuilder::new(&topology.id("registration:forwarding-context-h4"), &mock.url(""))
        .mode("inclusive")
        .entity_id(&entity_id)
        .entity_type("Veh")
        .property_names(&["spd"])
        .operations(&["createEntity"])
        .context_source_info("contentType", "application/ld+json")
        .context(registration_context(&client_context))
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("create registration"), 201);

    let inbound_context = registration_context(&client_context);
    let entity = json!({
        "id": entity_id,
        "type": "Veh",
        "spd": { "type": "Property", "value": 40 },
        "@context": inbound_context
    });
    let created = front
        .client()
        .raw(Method::POST, FORWARDED_ENTITIES)
        .header("Content-Type", "application/ld+json")
        .json(&entity)
        .send()
        .await
        .expect("create entity");
    assert!(created.status.is_success(), "the creation succeeds: {}", created.status);

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    assert!(
        context_link_targets(&forwarded).is_empty(),
        "an application/ld+json request carries no context link-value: {:?}",
        forwarded.headers.get_all("link")
    );
    let content_type = forwarded_header(&forwarded, "content-type").unwrap_or_default();
    assert!(content_type.starts_with("application/ld+json"), "the source receives JSON-LD: {content_type}");
    let body: Value = forwarded.json().expect("the forwarded entity is JSON");
    assert_eq!(body["@context"], inbound_context, "the inbound context travels inline: {body}");
    assert_eq!(body["type"], json!("Veh"), "the names stay in the inbound context's terms: {body}");
    assert_eq!(body["spd"]["value"], json!(40), "the names stay in the inbound context's terms: {body}");

    forget_cached(front.pool(), &[client_context]).await;
    mock.shutdown().await;
    origin.shutdown().await.expect("context server shuts down");
    topology.finish().await.expect("topology cleanup");
}

/// Review change 17: a relative context reference is never forwarded. An
/// `application/ld+json` source is sent the inbound context inline, but an
/// inbound context naming one of this broker's own hosted documents by its
/// root-relative path would resolve against the source's URL instead; the body
/// is then written inline against the core context, every term fully qualified
/// (clauses 4.3.6.6, 6.3.5).
#[serial_test::serial(distop_shared_type)]
#[tokio::test]
async fn distop_forwarding_context_a_relative_inline_reference_is_forwarded_against_the_core() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub_with_body(Method::POST, FORWARDED_ENTITIES, 201, &json!({})).await;

    let hosted = front
        .client()
        .raw(Method::POST, "/ngsi-ld/v1/jsonldContexts")
        .header("Content-Type", "application/json")
        .json(&json!({ "@context": { "spd": SPEED_IRI, "Veh": VEHICLE_IRI } }))
        .send()
        .await
        .expect("add @context");
    assert_status(&hosted, 201);
    let relative_reference = location(&hosted);
    assert!(
        relative_reference.starts_with('/'),
        "Add @context answers the local relative path (clause 6.29.3.1): {relative_reference}"
    );

    let entity_id = topology.id("Veh:forwarding-context-relative");
    let registration = CsrBuilder::new(&topology.id("registration:forwarding-context-relative"), &mock.url(""))
        .mode("inclusive")
        .entity_id(&entity_id)
        .entity_type(VEHICLE_IRI)
        .property_names(&[SPEED_IRI])
        .operations(&["createEntity"])
        .context_source_info("contentType", "application/ld+json")
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("create registration"), 201);

    let entity = json!({
        "id": entity_id,
        "type": "Veh",
        "spd": { "type": "Property", "value": 50 },
        "@context": relative_reference
    });
    let created = front
        .client()
        .raw(Method::POST, FORWARDED_ENTITIES)
        .header("Content-Type", "application/ld+json")
        .json(&entity)
        .send()
        .await
        .expect("create entity");
    assert!(
        created.status.is_success(),
        "the creation succeeds: {} {}",
        created.status,
        String::from_utf8_lossy(&created.body)
    );

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    assert!(
        context_link_targets(&forwarded).is_empty(),
        "an application/ld+json request carries no context link-value"
    );
    let body: Value = forwarded.json().expect("the forwarded entity is JSON");
    assert_eq!(body["@context"], json!(CORE_CONTEXT), "the inline context is the core context: {body}");
    assert_eq!(body["type"], json!(VEHICLE_IRI), "the type is fully qualified: {body}");
    assert_eq!(body[SPEED_IRI]["value"], json!(50), "the attribute is fully qualified: {body}");
    assert!(
        !body.to_string().contains(&relative_reference),
        "no relative reference reaches the source: {body}"
    );

    let deleted = front.client().raw(Method::DELETE, &relative_reference).send().await.expect("delete @context");
    assert_status(&deleted, 204);
    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Clause 6.3.19 sets `Content-Type` "whenever a payload body is present": a
/// query forwarded as a body-less GET to a registration whose `contentType` is
/// `application/ld+json` carries no `Content-Type`, so no JSON-LD media type
/// travels beside its context `Link` (clause 6.3.5).
#[serial_test::serial(distop_shared_type)]
#[tokio::test]
async fn distop_forwarding_context_a_body_less_forward_carries_no_content_type() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &json!([])).await;

    let origin = start_origin().await;
    let client_context = serve(&origin, &aliased_document()).await;

    let registration = CsrBuilder::new(&topology.id("registration:forwarding-context-body-less"), &mock.url(""))
        .mode("inclusive")
        .entity_type("Veh")
        .property_names(&["spd"])
        .operations(&["queryEntity"])
        .context_source_info("contentType", "application/ld+json")
        .context(registration_context(&client_context))
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("create registration"), 201);

    let response = front
        .client()
        .query_entities("type=Veh&attrs=spd", "application/ld+json", Some(&client_context))
        .await
        .expect("query");
    assert_status(&response, 200);

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    assert_eq!(forwarded.method, Method::GET);
    assert_eq!(
        forwarded_header(&forwarded, "content-type"),
        None,
        "a body-less forwarded request carries no Content-Type"
    );
    assert_single_context_link(&forwarded, &client_context);

    forget_cached(front.pool(), &[client_context]).await;
    mock.shutdown().await;
    origin.shutdown().await.expect("context server shuts down");
    topology.finish().await.expect("topology cleanup");
}

/// Clause 6.3.6: a 207 body carries Fully Qualified Names. A source written to
/// in a registration's configured `jsonldContext` answers a partial append
/// with an `UpdateResult` naming the attribute in that context (`velocity`);
/// the broker's own 207 names it by its full IRI, never in the source's
/// spelling (clauses 4.3.6.6, 5.2.18, 5.6.3.4).
#[serial_test::serial(distop_shared_type)]
#[tokio::test]
async fn distop_forwarding_context_a_forwarded_update_result_is_read_back_in_full_names() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Veh:forwarding-context-update-result");
    let attrs_path = format!("{FORWARDED_ENTITIES}/{entity_id}/attrs");
    mock.set_stub_with_body(
        Method::POST,
        &attrs_path,
        207,
        &json!({ "updated": [], "notUpdated": [{ "attributeName": "velocity", "reason": "the source already holds it" }] }),
    )
    .await;

    let origin = start_origin().await;
    let client_context = serve(&origin, &aliased_document()).await;
    let configured_context = serve(&origin, &configured_document()).await;

    let registration = CsrBuilder::new(&topology.id("registration:forwarding-context-update-result"), &mock.url(""))
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["spd"])
        .operations(&["appendAttrs", "retrieveEntity"])
        .context_source_info("jsonldContext", &configured_context)
        .context(registration_context(&client_context))
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("create registration"), 201);

    let response = front
        .client()
        .raw(Method::POST, &attrs_path)
        .header("Content-Type", "application/json")
        .header("Link", &context_link(&client_context))
        .json(&json!({ "spd": { "type": "Property", "value": 9 } }))
        .send()
        .await
        .expect("append attributes");
    assert_status(&response, 207);

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    let forwarded_body: Value = forwarded.json().expect("the forwarded fragment is JSON");
    assert!(
        forwarded_body.get("velocity").is_some(),
        "the fragment is spelled in the configured context: {forwarded_body}"
    );

    let answer = response.json_value().expect("the 207 body is JSON");
    let names = answer["notUpdated"]
        .as_array()
        .expect("an UpdateResult lists what was not updated")
        .iter()
        .map(|details| details["attributeName"].clone()) // clone: the assertion owns the names
        .collect::<Vec<_>>();
    assert!(names.contains(&json!(SPEED_IRI)), "the attribute is named by its full IRI: {answer}");
    assert!(!names.contains(&json!("velocity")), "the source's spelling does not leak: {answer}");

    forget_cached(front.pool(), &[client_context, configured_context]).await;
    mock.shutdown().await;
    origin.shutdown().await.expect("context server shuts down");
    topology.finish().await.expect("topology cleanup");
}

/// An attribute whose term is `attrs` addresses `/entities/{id}/attrs/attrs`;
/// only the attribute segment -- the one after the `attrs` collection -- is
/// spelled in the registration's configured context, so the source receives
/// `/attrs/flags` and never `/flags/flags` (clauses 4.3.6.6, 6.7.3.2).
#[serial_test::serial(distop_shared_type)]
#[tokio::test]
async fn distop_forwarding_context_an_attribute_named_attrs_is_spelled_by_its_position() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Veh:forwarding-context-attrs-segment");
    let forwarded_path = format!("{FORWARDED_ENTITIES}/{entity_id}/attrs/flags");
    mock.set_stub(Method::DELETE, &forwarded_path, 204).await;

    let origin = start_origin().await;
    let client_context = serve(&origin, &json!({ "@context": { "attrs": ATTRS_IRI, "Veh": VEHICLE_IRI } }).to_string()).await;
    let configured_context = serve(&origin, &json!({ "@context": { "flags": ATTRS_IRI, "Car": VEHICLE_IRI } }).to_string()).await;

    let registration = CsrBuilder::new(&topology.id("registration:forwarding-context-attrs-segment"), &mock.url(""))
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["attrs"])
        .operations(&["deleteAttrs", "retrieveEntity"])
        .context_source_info("jsonldContext", &configured_context)
        .context(registration_context(&client_context))
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("create registration"), 201);

    let response = front
        .client()
        .raw(Method::DELETE, &format!("{FORWARDED_ENTITIES}/{entity_id}/attrs/attrs"))
        .header("Link", &context_link(&client_context))
        .send()
        .await
        .expect("delete attribute");
    assert!(
        response.status.is_success(),
        "the deletion succeeds: {} {}",
        response.status,
        String::from_utf8_lossy(&response.body)
    );

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    assert_eq!(forwarded.uri.path(), forwarded_path, "only the attribute segment is respelled");

    forget_cached(front.pool(), &[client_context, configured_context]).await;
    mock.shutdown().await;
    origin.shutdown().await.expect("context server shuts down");
    topology.finish().await.expect("topology cleanup");
}

/// H5: a batch creation whose items carry different contexts -- each JSON-LD
/// node is read under its own `@context` -- is forwarded as one JSON request
/// against the core context: never a second context link-value, never a
/// relative one, and every item's attribute fully qualified whichever term the
/// client used for it (clauses 5.6.7, 6.3.5).
#[serial_test::serial(distop_shared_type)]
#[tokio::test]
async fn distop_forwarding_context_a_mixed_context_batch_carries_one_absolute_context_at_most() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub(Method::POST, FORWARDED_BATCH_CREATE, 201).await;

    let origin = start_origin().await;
    let client_context = serve(&origin, &aliased_document()).await;
    let rate_context = serve(&origin, &rate_document()).await;

    let registration = CsrBuilder::new(&topology.id("registration:forwarding-context-h5"), &mock.url(""))
        .mode("inclusive")
        .entity_type("Veh")
        .id_pattern(&format!("^{}", topology.id_prefix()))
        .property_names(&["spd"])
        .operations(&["createBatch"])
        .context(registration_context(&client_context))
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("create registration"), 201);

    let first = topology.id("Veh:forwarding-context-h5-a");
    let second = topology.id("Veh:forwarding-context-h5-b");
    let batch = json!([
        { "id": first, "type": "Veh", "spd": { "type": "Property", "value": 1 }, "@context": client_context },
        { "id": second, "type": "Veh", "rate": { "type": "Property", "value": 2 }, "@context": rate_context }
    ]);
    let created = front
        .client()
        .raw(Method::POST, FORWARDED_BATCH_CREATE)
        .header("Content-Type", "application/ld+json")
        .json(&batch)
        .send()
        .await
        .expect("batch create");
    assert!(created.status.is_success(), "the batch creation succeeds: {}", created.status);

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    assert_eq!(forwarded.uri.path(), FORWARDED_BATCH_CREATE);
    assert_core_or_no_context_link(&forwarded);
    let body: Value = forwarded.json().expect("the forwarded batch is JSON");
    let items = body.as_array().expect("the forwarded batch is an array");
    assert_eq!(items.len(), 2, "both items reach the source: {body}");
    for item in items {
        assert!(item.get("@context").is_none(), "a JSON batch item carries no inline @context: {item}");
        assert_eq!(item["type"], json!(VEHICLE_IRI), "each item's type is fully qualified: {item}");
        assert!(item.get(SPEED_IRI).is_some(), "each item's attribute is fully qualified: {item}");
    }

    forget_cached(front.pool(), &[client_context, rate_context]).await;
    mock.shutdown().await;
    origin.shutdown().await.expect("context server shuts down");
    topology.finish().await.expect("topology cleanup");
}
