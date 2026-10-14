use crate::{
    forwarded_content::AttributeWrite,
    forwarded_payload::{ForwardedPayload, ItemBody, OutgoingBody, build_forwarded_payload, build_identifiers_payload, build_items_payload},
    forwarding_context::{BodyPresence, ForwardingContext, Rewording, decide, decide_for_items},
};
use andromeda_http_headers::media_type::ResponseMediaType;
use andromeda_json_literal::JsonLiteral;
use andromeda_jsonld::{batch_contexts::BatchContexts, context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_entity::document::{EntityDocument, attribute_member::AttributeMember};
use andromeda_ngsi_ld_expansion::{entity_expansion::EntityExpansion, normalization::Normalization};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::{
    attribute::{instance::AttributeValue, member_reading::MemberReading, merge_member::MergeMember},
    context_source_info::ContextSourceInfo,
    entity::EntityPatch,
    key_value_pair::KeyValuePair,
    query::body::Query,
    subscription_request::SubscriptionRequest,
};
use andromeda_ngsi_ld_render::version::adapted_payload::AdaptedPayload;
use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
use indexmap::IndexMap;
use iri_rs::IriBuf;
use serde_json::{Value, json};
use std::{
    borrow::Cow,
    collections::HashMap,
    env,
    fs,
    process,
    sync::atomic::{AtomicU64, Ordering},
};
use url::Url;

/// Makes every temporary context file name unique, so tests running in
/// parallel never write or remove each other's files.
static CONTEXT_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

const USER_CONTEXT: &str = "https://example.org/user-context.jsonld";
const OTHER_CONTEXT: &str = "https://example.org/other-context.jsonld";
const CONFIGURED_CONTEXT: &str = "https://example.org/configured-context.jsonld";

/// An engine that resolves the three fixture contexts from temporary files
/// and never the network. `speed` is `https://example.org/speed` in the
/// user context and `velocity` in the configured one.
fn engine() -> JsonLdEngine {
    let documents = [
        (
            USER_CONTEXT,
            json!({"@context": {"speed": "https://example.org/speed", "Vehicle": "https://example.org/Vehicle"}}),
        ),
        (OTHER_CONTEXT, json!({"@context": {"brand": "https://example.org/brand"}})),
        (
            CONFIGURED_CONTEXT,
            json!({"@context": {"velocity": "https://example.org/speed", "Car": "https://example.org/Vehicle"}}),
        ),
    ];
    let mut preloads = HashMap::new();
    let mut paths = Vec::new();
    for (url, document) in &documents {
        let sequence = CONTEXT_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = env::temp_dir().join(format!("andromeda-forwarded-payload-{}-{sequence}.jsonld", process::id()));
        fs::write(&path, document.to_string()).expect("the temporary directory is writable");
        preloads.insert(Url::parse(url).expect("a valid URL"), path.clone()); // clone: removed below
        paths.push(path);
    }
    let engine = JsonLdEngine::builder()
        .allow_remote(false)
        .preload_contexts(preloads)
        .build()
        .expect("offline engine");
    for path in paths {
        fs::remove_file(&path).expect("the temporary context is removable");
    }
    engine
}

fn pairs(entries: &[(&str, &str)]) -> Vec<KeyValuePair> {
    entries
        .iter()
        .map(|(key, value)| KeyValuePair {
            key: (*key).into(),
            value: (*value).into(),
        })
        .collect()
}

fn link_spec(url: &str) -> ContextSpec {
    ContextSpec::from_link(IriBuf::new(url.to_owned()).expect("valid IRI"))
}

fn literal(context: &Value) -> JsonLiteral {
    JsonLiteral::admit(&context.to_string()).expect("an admissible literal")
}

/// The document the admission expands `written` to under `spec`.
async fn expanded(spec: &ContextSpec, written: &Value) -> EntityDocument {
    let engine = engine();
    let expansion = EntityExpansion::prepare(&engine, spec).await.expect("the context processes");
    let patch: EntityPatch = serde_json::from_value(written.clone()).expect("a partial entity"); // clone: the fixture keeps its written form
    expansion.entity_patch(&patch, Normalization::Entity).await.expect("the entity expands")
}

fn vehicle() -> Value {
    json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "speed": {"type": "Property", "value": 10}})
}

fn body_context(payload_body: &[u8]) -> Option<JsonLiteral> {
    serde_json::from_slice::<Value>(payload_body)
        .expect("JSON body")
        .get("@context")
        .map(|context| JsonLiteral::admit(&context.to_string()).expect("an admissible context"))
}

fn forwarded_json(payload: &ForwardedPayload) -> Value {
    serde_json::from_slice(payload.body.as_deref().expect("body")).expect("JSON body")
}

async fn payload(inbound: &ContextSpec, entries: &[(&str, &str)], body: Option<OutgoingBody<'_>>) -> ForwardedPayload {
    let raw = pairs(entries);
    let info = ContextSourceInfo::try_from(raw.as_slice()).expect("valid info");
    let presence = if body.is_some() { BodyPresence::Present } else { BodyPresence::Absent };
    let decision = decide(inbound, &info, presence).expect("decides");
    build_forwarded_payload(&engine(), inbound, &decision, body, &info)
        .await
        .expect("payload builds")
}

/// An entity document of the Element `AdaptedPayload::Entity`.
const fn entity(document: &EntityDocument) -> OutgoingBody<'_> {
    OutgoingBody::Entity {
        document,
        element: AdaptedPayload::Entity,
    }
}

#[tokio::test]
async fn a_link_context_is_forwarded_as_application_json_with_the_same_link_and_no_inline_context() {
    let inbound = link_spec(USER_CONTEXT);
    let document = expanded(&inbound, &vehicle()).await;
    let payload = payload(&inbound, &[], Some(entity(&document))).await;
    pretty_assertions::assert_eq!(payload.media_type, ResponseMediaType::Json);
    pretty_assertions::assert_eq!(payload.link_context.as_ref().map(IriBuf::as_str), Some(USER_CONTEXT));
    let forwarded = forwarded_json(&payload);
    assert!(forwarded.get("@context").is_none());
    assert!(forwarded.get("speed").is_some(), "the client's terms travel unchanged: {forwarded}");
}

#[tokio::test]
async fn a_content_type_of_ld_json_inlines_the_context_and_sends_no_link() {
    let inbound = link_spec(USER_CONTEXT);
    let document = expanded(&inbound, &vehicle()).await;
    let payload = payload(&inbound, &[("contentType", "application/ld+json")], Some(entity(&document))).await;
    pretty_assertions::assert_eq!(payload.media_type, ResponseMediaType::JsonLd);
    assert!(payload.link_context.is_none());
    pretty_assertions::assert_eq!(body_context(payload.body.as_deref().expect("body")), Some(literal(&json!(USER_CONTEXT))));
}

#[tokio::test]
async fn an_inline_url_context_travels_as_a_link_under_application_json() {
    let inbound = ContextSpec::from_body(literal(&json!(USER_CONTEXT)));
    let document = expanded(&inbound, &vehicle()).await;
    let payload = payload(&inbound, &[], Some(entity(&document))).await;
    pretty_assertions::assert_eq!(payload.media_type, ResponseMediaType::Json);
    pretty_assertions::assert_eq!(payload.link_context.as_ref().map(IriBuf::as_str), Some(USER_CONTEXT));
    assert!(body_context(payload.body.as_deref().expect("body")).is_none());
}

/// JSON-LD 1.1 section 6.1: several inbound context URLs are never forwarded
/// as several context links. Under `application/json` the body is compacted
/// against the one core context link, which writes a user term as its full
/// IRI; under `application/ld+json` the URLs travel inline.
#[tokio::test]
async fn several_inbound_urls_are_forwarded_under_one_core_link_with_full_names() {
    let inbound = ContextSpec::from_body(literal(&json!([USER_CONTEXT, OTHER_CONTEXT])));
    let document = expanded(&inbound, &vehicle()).await;
    let as_json = payload(&inbound, &[], Some(entity(&document))).await;
    pretty_assertions::assert_eq!(as_json.link_context.as_ref().map(IriBuf::as_str), Some(NGSI_LD_CORE_CONTEXT_URL.as_str()));
    let forwarded = forwarded_json(&as_json);
    assert!(forwarded.get("@context").is_none());
    pretty_assertions::assert_eq!(forwarded["type"], json!("https://example.org/Vehicle"));
    pretty_assertions::assert_eq!(forwarded["https://example.org/speed"]["value"], json!(10));
    pretty_assertions::assert_eq!(forwarded["id"], json!("urn:ngsi-ld:Vehicle:1"));

    let as_ld = payload(&inbound, &[("contentType", "application/ld+json")], Some(entity(&document))).await;
    assert!(as_ld.link_context.is_none());
    pretty_assertions::assert_eq!(
        body_context(as_ld.body.as_deref().expect("body")),
        Some(literal(&json!([USER_CONTEXT, OTHER_CONTEXT])))
    );
}

#[tokio::test]
async fn an_inline_object_context_is_re_inlined_for_ld_json_and_compacted_under_the_core_link_for_json() {
    let inline = literal(&json!({"speed": "https://example.org/speed", "Vehicle": "https://example.org/Vehicle"}));
    let inbound = ContextSpec::from_body(inline.clone()); // clone: compared below
    let document = expanded(&inbound, &vehicle()).await;

    let as_ld = payload(&inbound, &[("contentType", "application/ld+json")], Some(entity(&document))).await;
    pretty_assertions::assert_eq!(as_ld.media_type, ResponseMediaType::JsonLd);
    assert!(as_ld.link_context.is_none());
    pretty_assertions::assert_eq!(body_context(as_ld.body.as_deref().expect("body")), Some(inline));

    let as_json = payload(&inbound, &[], Some(entity(&document))).await;
    pretty_assertions::assert_eq!(as_json.link_context.as_ref().map(IriBuf::as_str), Some(NGSI_LD_CORE_CONTEXT_URL.as_str()));
    let forwarded = forwarded_json(&as_json);
    pretty_assertions::assert_eq!(forwarded["https://example.org/speed"]["type"], json!("Property"));
    assert!(forwarded.get("@context").is_none());
}

/// Clause 4.3.6.6: a configured `jsonldContext` forces `application/json`,
/// names that context in the one `Link`, and the body is compacted against
/// it -- `speed` becomes `velocity` and `Vehicle` becomes `Car`, which only
/// the configured context spells that way.
#[tokio::test]
async fn a_forwarded_entity_is_spelled_by_compaction() {
    let inbound = link_spec(USER_CONTEXT);
    let document = expanded(&inbound, &vehicle()).await;
    let payload = payload(
        &inbound,
        &[("jsonldContext", CONFIGURED_CONTEXT), ("contentType", "application/ld+json")],
        Some(entity(&document)),
    )
    .await;
    pretty_assertions::assert_eq!(payload.media_type, ResponseMediaType::Json);
    pretty_assertions::assert_eq!(payload.link_context.as_ref().map(IriBuf::as_str), Some(CONFIGURED_CONTEXT));
    let forwarded = forwarded_json(&payload);
    assert!(forwarded.get("@context").is_none());
    pretty_assertions::assert_eq!(forwarded["type"], json!("Car"));
    pretty_assertions::assert_eq!(forwarded["velocity"]["value"], json!(10));
    assert!(forwarded.get("speed").is_none(), "{forwarded}");
}

/// A body this broker already wrote in the forwarding context is sent as
/// written.
#[tokio::test]
async fn an_expressed_query_is_never_respelled() {
    let query: Query = serde_json::from_value(json!({"type": "Query", "entities": [{"type": "Car"}], "attrs": ["velocity"]})).expect("a query");
    let payload = payload(
        &link_spec(USER_CONTEXT),
        &[("jsonldContext", CONFIGURED_CONTEXT)],
        Some(OutgoingBody::Query(&query)),
    )
    .await;
    pretty_assertions::assert_eq!(
        forwarded_json(&payload),
        json!({"type": "Query", "entities": [{"type": "Car"}], "attrs": ["velocity"]})
    );
}

/// A reduced subscription is respelled from its typed members rather than
/// through a JSON-LD round trip, so its shape survives.
#[tokio::test]
async fn a_forwarded_subscription_is_spelled_from_the_typed_subscription() {
    let subscription: SubscriptionRequest = serde_json::from_value(json!({
        "id": "urn:ngsi-ld:Subscription:forwarded-1",
        "type": "Subscription",
        "entities": [{"type": "Vehicle"}],
        "watchedAttributes": ["speed"],
        "q": "speed>1",
        "notification": {"endpoint": {"uri": "http://receiver.example.org/"}}
    }))
    .expect("a subscription request");
    let payload = payload(
        &link_spec(USER_CONTEXT),
        &[("jsonldContext", CONFIGURED_CONTEXT)],
        Some(OutgoingBody::Subscription(&subscription)),
    )
    .await;
    let forwarded = forwarded_json(&payload);
    pretty_assertions::assert_eq!(forwarded["entities"], json!([{"type": "Car"}]));
    pretty_assertions::assert_eq!(forwarded["watchedAttributes"], json!(["velocity"]));
    pretty_assertions::assert_eq!(forwarded["q"], json!("velocity>1"));
}

/// A relative reference is never forwarded. An inline
/// context holding one -- as an entry, or as an `@import` -- would resolve
/// against the source's URL rather than this broker's request, so the
/// `application/ld+json` body is written inline against the core context
/// instead, every user term as its full IRI.
#[tokio::test]
async fn an_inline_context_holding_a_relative_reference_is_never_forwarded() {
    let query: Query = serde_json::from_value(json!({"type": "Query", "entities": [{"type": "https://example.org/Vehicle"}]})).expect("a query");
    let ld = [("contentType", "application/ld+json")];
    for relative in [
        json!(["ctx.jsonld", USER_CONTEXT]),
        json!("/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:JsonLdContext:1"),
        json!([USER_CONTEXT, {"@import": "shared/terms.jsonld", "brand": "https://example.org/brand"}]),
    ] {
        let inbound = ContextSpec::from_body(literal(&relative));
        let raw = pairs(&ld);
        let info = ContextSourceInfo::try_from(raw.as_slice()).expect("valid info");
        let decision = decide(&inbound, &info, BodyPresence::Present).expect("decides");
        pretty_assertions::assert_eq!(decision.context, ForwardingContext::InlineCore);
        pretty_assertions::assert_eq!(decision.context.rewording(), Rewording::Reword);
        pretty_assertions::assert_eq!(decision.context.naming(&inbound), ContextSpec::none());

        let forwarded = payload(&inbound, &ld, Some(OutgoingBody::Query(&query))).await;
        pretty_assertions::assert_eq!(forwarded.media_type, ResponseMediaType::JsonLd);
        assert!(forwarded.link_context.is_none());
        pretty_assertions::assert_eq!(
            body_context(forwarded.body.as_deref().expect("body")),
            Some(literal(&json!(NGSI_LD_CORE_CONTEXT_URL.as_str())))
        );
    }
}

/// An inline context whose every reference is absolute travels as written,
/// including an absolute `@import`.
#[tokio::test]
async fn an_inline_context_of_absolute_references_travels_as_written() {
    let absolute = literal(&json!([USER_CONTEXT, {"@import": OTHER_CONTEXT, "colour": "https://example.org/colour"}]));
    let inbound = ContextSpec::from_body(absolute.clone()); // clone: compared below
    let document = expanded(&inbound, &vehicle()).await;
    let forwarded = payload(&inbound, &[("contentType", "application/ld+json")], Some(entity(&document))).await;
    pretty_assertions::assert_eq!(body_context(forwarded.body.as_deref().expect("body")), Some(absolute));
}

/// The batch payload of one registration, each item under its own context.
async fn items_payload(entries: &[(&str, &str)], items: Vec<ItemBody<'_>>) -> ForwardedPayload {
    let raw = pairs(entries);
    let info = ContextSourceInfo::try_from(raw.as_slice()).expect("valid info");
    let decision = decide_for_items(items.iter().map(|item| item.context), &info).expect("decides");
    build_items_payload(&engine(), &decision, items, &info).await.expect("payload builds")
}

/// Items submitted under different contexts have no single context a `Link`
/// can name: under `application/json` the batch names the core context
/// once, and each item is spelled from its own context as full IRIs.
#[tokio::test]
async fn items_under_different_contexts_are_forwarded_under_one_core_link_each_from_its_own_context() {
    let mut contexts = BatchContexts::per_entity();
    let user = contexts.context_of(Some(literal(&json!(USER_CONTEXT))));
    let configured = contexts.context_of(Some(literal(&json!(CONFIGURED_CONTEXT))));
    let first = expanded(
        user.spec(),
        &json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "speed": {"type": "Property", "value": 1}}),
    )
    .await;
    let second = expanded(
        configured.spec(),
        &json!({"id": "urn:ngsi-ld:Vehicle:2", "type": "Car", "velocity": {"type": "Property", "value": 2}}),
    )
    .await;
    let items = vec![
        ItemBody {
            document: Cow::Owned(first),
            context: &user,
        },
        ItemBody {
            document: Cow::Owned(second),
            context: &configured,
        },
    ];
    let forwarded = items_payload(&[], items).await;
    pretty_assertions::assert_eq!(forwarded.media_type, ResponseMediaType::Json);
    pretty_assertions::assert_eq!(forwarded.link_context.as_ref().map(IriBuf::as_str), Some(NGSI_LD_CORE_CONTEXT_URL.as_str()));
    let body = forwarded_json(&forwarded);
    for (index, value) in [(0, 1), (1, 2)] {
        pretty_assertions::assert_eq!(body[index]["type"], json!("https://example.org/Vehicle"), "{body}");
        pretty_assertions::assert_eq!(body[index]["https://example.org/speed"]["value"], json!(value), "{body}");
        assert!(body[index].get("@context").is_none(), "{body}");
    }
}

/// Under `application/ld+json` each item carries its own context inline,
/// and the batch names no context `Link` (clause 6.3.5): a forwarded batch
/// spells each item under its own context.
#[tokio::test]
async fn a_forwarded_batch_spells_each_item_under_its_context() {
    let mut contexts = BatchContexts::per_entity();
    let user = contexts.context_of(Some(literal(&json!(USER_CONTEXT))));
    let configured = contexts.context_of(Some(literal(&json!(CONFIGURED_CONTEXT))));
    let first = expanded(user.spec(), &json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle"})).await;
    let second = expanded(configured.spec(), &json!({"id": "urn:ngsi-ld:Vehicle:2", "type": "Car"})).await;
    let items = vec![
        ItemBody {
            document: Cow::Owned(first),
            context: &user,
        },
        ItemBody {
            document: Cow::Owned(second),
            context: &configured,
        },
    ];
    let forwarded = items_payload(&[("contentType", "application/ld+json")], items).await;
    assert!(forwarded.link_context.is_none());
    let body = forwarded_json(&forwarded);
    pretty_assertions::assert_eq!(body[0]["@context"], json!(USER_CONTEXT));
    pretty_assertions::assert_eq!(body[1]["@context"], json!(CONFIGURED_CONTEXT));
    pretty_assertions::assert_eq!(body[0]["type"], json!("Vehicle"));
    pretty_assertions::assert_eq!(body[1]["type"], json!("Car"));
}

/// Clause 6.3.5: a forwarded batch deletion's bare identifiers cannot carry
/// an `@context`, so they travel as `application/json` under no context
/// `Link`, in array order.
#[test]
fn a_batch_deletion_forwards_its_identifiers_as_json_under_no_link() {
    let ids = ["urn:ngsi-ld:Vehicle:1", "urn:ngsi-ld:Vehicle:2"].map(|raw| raw.parse::<Identifier>().expect("a valid identifier"));
    let forwarded = build_identifiers_payload(&ids).expect("payload builds");
    pretty_assertions::assert_eq!(forwarded.media_type, ResponseMediaType::Json);
    assert!(forwarded.link_context.is_none());
    pretty_assertions::assert_eq!(forwarded_json(&forwarded), json!(["urn:ngsi-ld:Vehicle:1", "urn:ngsi-ld:Vehicle:2"]));
}

/// An item whose own context holds a relative reference cannot travel
/// inline, so the whole batch is written inline against the core context.
#[test]
fn items_under_ld_json_with_a_relative_reference_fall_back_to_the_inline_core_context() {
    let mut contexts = BatchContexts::per_entity();
    let user = contexts.context_of(Some(literal(&json!(USER_CONTEXT))));
    let relative = contexts.context_of(Some(literal(&json!(["relative.jsonld", USER_CONTEXT]))));
    let other = contexts.context_of(Some(literal(&json!(OTHER_CONTEXT))));
    let raw = pairs(&[("contentType", "application/ld+json")]);
    let info = ContextSourceInfo::try_from(raw.as_slice()).expect("valid info");

    let decision = decide_for_items([&user, &relative], &info).expect("decides");
    pretty_assertions::assert_eq!(decision.context, ForwardingContext::InlineCore);
    pretty_assertions::assert_eq!(
        decision.context.inline_for(user.spec()).expect("an inline context"),
        Some(literal(&json!(NGSI_LD_CORE_CONTEXT_URL.as_str())))
    );
    pretty_assertions::assert_eq!(decision.context.naming(relative.spec()), ContextSpec::none());

    let absolute_only = decide_for_items([&user, &other], &info).expect("decides");
    pretty_assertions::assert_eq!(absolute_only.context, ForwardingContext::InlinePerItem);
}

#[tokio::test]
async fn a_body_less_request_carries_only_its_one_link() {
    let payload_with_link = payload(&link_spec(USER_CONTEXT), &[], None).await;
    assert!(payload_with_link.body.is_none());
    pretty_assertions::assert_eq!(payload_with_link.link_context.as_ref().map(IriBuf::as_str), Some(USER_CONTEXT));

    let none = payload(&ContextSpec::none(), &[], None).await;
    assert!(none.link_context.is_none());
}

/// The body forwarded to a 1.0 source for `body`.
async fn forwarded_to_1_0(body: OutgoingBody<'_>) -> Value {
    forwarded_json(&payload(&link_spec(USER_CONTEXT), &[("ngsildConformance", "1.0")], Some(body)).await)
}

#[tokio::test]
async fn an_ngsild_conformance_applies_the_clause_4_3_6_8_fallbacks() {
    let inbound = link_spec(USER_CONTEXT);
    let document = expanded(
        &inbound,
        &json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "scope": "/Madrid",
            "speed": {"type": "Property", "value": 10, "observedAt": "2024-01-01T00:00:00Z"}
        }),
    )
    .await;
    let forwarded = forwarded_to_1_0(entity(&document)).await;
    assert!(forwarded.get("scope").is_none(), "scope was introduced in 1.4: {forwarded}");
    assert!(forwarded["speed"].get("observedAt").is_none(), "observedAt was introduced in 1.3: {forwarded}");
}

/// Tables 4.3.6.8-x describe Entity data alone: a forwarded Subscription
/// keeps its own `expiresAt` and every `entities` selector.
#[tokio::test]
async fn a_forwarded_subscription_is_not_read_as_an_entity() {
    let subscription: SubscriptionRequest = serde_json::from_value(json!({
        "id": "urn:ngsi-ld:Subscription:forwarded-1",
        "type": "Subscription",
        "entities": [{"type": "Vehicle"}, {"type": "Bus"}],
        "expiresAt": "2030-01-01T00:00:00Z",
        "notification": {"endpoint": {"uri": "http://receiver.example.org/"}}
    }))
    .expect("a subscription request");
    let forwarded = forwarded_to_1_0(OutgoingBody::Subscription(&subscription)).await;
    pretty_assertions::assert_eq!(forwarded["expiresAt"], json!("2030-01-01T00:00:00Z"));
    pretty_assertions::assert_eq!(forwarded["entities"], json!([{"type": "Vehicle"}, {"type": "Bus"}]));
}

/// A batch `Query` is no Entity data either.
#[tokio::test]
async fn a_forwarded_query_keeps_its_entity_selectors() {
    let query = json!({"type": "Query", "entities": [{"type": "Vehicle"}, {"type": "Bus"}]});
    let typed: Query = serde_json::from_value(query.clone()).expect("a query"); // clone: compared below
    pretty_assertions::assert_eq!(forwarded_to_1_0(OutgoingBody::Query(&typed)).await, query);
}

/// Tables 4.3.6.8-2 and -3 reach the instance members of the one attribute
/// a request path names.
#[tokio::test]
async fn a_forwarded_attribute_body_drops_its_newer_members() {
    let inbound = link_spec(USER_CONTEXT);
    let document = expanded(
        &inbound,
        &json!({"speed": {"type": "Property", "value": 10, "observedAt": "2025-01-01T00:00:00Z", "unitCode": "KMH", "valueType": "Speed"}}),
    )
    .await;
    let (name, member) = document.attributes().first().expect("one attribute");
    let instance = member.instances().first().expect("one instance");
    let forwarded = forwarded_to_1_0(OutgoingBody::Attribute {
        name,
        instance,
        write: AttributeWrite::Replacement,
    })
    .await;
    pretty_assertions::assert_eq!(forwarded, json!({"type": "Property", "value": 10}));
}

/// Clause 4.5.7: a temporal entity's attribute array is the sequence of
/// its instances in time, and `observedAt` places each in it.
#[tokio::test]
async fn a_forwarded_temporal_body_keeps_its_instance_sequence() {
    let speeds = json!([
        {"type": "Property", "value": 1, "observedAt": "2025-01-01T00:00:00Z"},
        {"type": "Property", "value": 2, "observedAt": "2025-01-02T00:00:00Z"},
        {"type": "Property", "value": 3, "observedAt": "2025-01-03T00:00:00Z"}
    ]);
    let document = expanded(
        &link_spec(USER_CONTEXT),
        &json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "speed": speeds}),
    )
    .await;
    let forwarded = forwarded_to_1_0(OutgoingBody::Entity {
        document: &document,
        element: AdaptedPayload::TemporalEvolution,
    })
    .await;
    pretty_assertions::assert_eq!(forwarded["speed"], speeds);
}

/// The document holding the reified Scope instances `written`, under the
/// Scope IRI, as the temporal write expands them.
async fn scope_history(spec: &ContextSpec, written: &[Value]) -> EntityDocument {
    let engine = engine();
    let expansion = EntityExpansion::prepare(&engine, spec).await.expect("the context processes");
    let name = "scope".parse().expect("an attribute name");
    let mut instances = Vec::new();
    for instance in written {
        let member: AttributeValue = serde_json::from_value(instance.clone()).expect("an instance"); // clone: the fixture keeps its written form
        instances.push(expansion.attribute(&name, &member).await.expect("the instance expands").into_parts().1);
    }
    let scope = "https://uri.etsi.org/ngsi-ld/scope".parse().expect("an attribute name");
    EntityDocument::new(None, Vec::new(), None, None, IndexMap::from([(scope, AttributeMember::Instances(instances))]))
}

/// Clauses 4.5.6 and 5.6.11.4: a reified Scope is forwarded as the sequence of
/// its instances, each with its temporal sub-properties, beside the instances
/// the evolution adds; a source conforming to 1.0 reads no `scope`.
#[tokio::test]
async fn a_forwarded_temporal_body_carries_its_reified_scope() {
    let document = expanded(
        &link_spec(USER_CONTEXT),
        &json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "speed": [{"type": "Property", "value": 1, "observedAt": "2025-01-01T00:00:00Z"}]}),
    )
    .await;
    let history = scope_history(
        &link_spec(USER_CONTEXT),
        &[
            json!({"type": "Property", "value": "/Madrid", "observedAt": "2025-01-01T00:00:00Z", "createdAt": "2025-01-01T00:00:01Z"}),
            json!({"type": "Property", "value": "urn:ngsi-ld:null", "observedAt": "2025-01-02T00:00:00Z", "deletedAt": "2025-01-02T00:00:00Z"}),
        ],
    )
    .await;
    let body = || OutgoingBody::Evolution {
        document: &document,
        scope_history: &history,
        element: AdaptedPayload::TemporalEvolution,
    };
    let forwarded = forwarded_json(&payload(&link_spec(USER_CONTEXT), &[], Some(body())).await);
    let scope = forwarded["scope"].as_array().expect("the scope is a sequence");
    pretty_assertions::assert_eq!(scope.len(), 2);
    pretty_assertions::assert_eq!(scope[0]["value"], json!("/Madrid"));
    pretty_assertions::assert_eq!(scope[0]["createdAt"], json!("2025-01-01T00:00:01Z"));
    pretty_assertions::assert_eq!(scope[1]["deletedAt"], json!("2025-01-02T00:00:00Z"));
    assert!(forwarded.get("speed").is_some(), "{forwarded}");
    assert!(forwarded_to_1_0(body()).await.get("scope").is_none());
}

/// Entity data is amended: `scope` arrived at 1.4 and the
/// `LanguageProperty` at 1.4.
#[tokio::test]
async fn a_forwarded_entity_is_amended() {
    let document = expanded(
        &link_spec(USER_CONTEXT),
        &json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "scope": "/Madrid",
            "name": {"type": "LanguageProperty", "languageMap": {"en": "bus"}}
        }),
    )
    .await;
    let forwarded = forwarded_to_1_0(entity(&document)).await;
    assert!(forwarded.get("scope").is_none(), "{forwarded}");
    pretty_assertions::assert_eq!(forwarded["name"], json!({"type": "Property", "value": {"en": "bus"}}));
}

/// The one attribute a request path names is spelled as an attribute in
/// the forwarding context: its core members stay, what it names moves.
#[tokio::test]
async fn an_attribute_body_is_spelled_as_an_attribute_in_the_configured_context() {
    let inbound = link_spec(USER_CONTEXT);
    let document = expanded(
        &inbound,
        &json!({"status": {"type": "Property", "value": 10, "speed": {"type": "Property", "value": 1}}}),
    )
    .await;
    let (name, member) = document.attributes().first().expect("one attribute");
    let instance = member.instances().first().expect("one instance");
    let payload = payload(
        &inbound,
        &[("jsonldContext", CONFIGURED_CONTEXT)],
        Some(OutgoingBody::Attribute {
            name,
            instance,
            write: AttributeWrite::Replacement,
        }),
    )
    .await;
    pretty_assertions::assert_eq!(
        forwarded_json(&payload),
        json!({"type": "Property", "value": 10, "velocity": {"type": "Property", "value": 1}})
    );
}

/// Clause 5.6.4: the Attribute Fragment of a Partial Attribute Update reaches
/// the source as the client wrote it. A fragment naming neither a `type` nor a
/// value member is the instance it updates, so the `speed` it holds stays a
/// member of that instance and is never nested under a `value`.
#[tokio::test]
async fn a_partial_update_fragment_is_forwarded_as_the_client_wrote_it() {
    let inbound = link_spec(USER_CONTEXT);
    let written = json!({"speed": {"type": "Property", "value": 99, "source": {"type": "Property", "value": "Speedometer"}}});
    let text = JsonLiteral::admit(&written.to_string()).expect("a JSON text");
    let member = MergeMember::from_fragment_node("speed", text.piece()).expect("a fragment");
    let name: AttributeName = "speed".parse().expect("an attribute name");
    let engine = engine();
    let expansion = EntityExpansion::prepare(&engine, &inbound).await.expect("the context processes");
    let fragment = expansion.attribute(&name, &member).await.expect("the fragment expands");
    let payload = payload(
        &inbound,
        &[],
        Some(OutgoingBody::Attribute {
            name: fragment.name(),
            instance: fragment.instance(),
            write: AttributeWrite::Fragment,
        }),
    )
    .await;
    pretty_assertions::assert_eq!(forwarded_json(&payload), written);
}
