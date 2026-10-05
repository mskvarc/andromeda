//! The representation a forwarded request carries (clauses 4.3.6.6, 4.3.6.8, 6.3.5, 6.3.19).
//!
//! A request arriving at this broker carries its `@context` in one of the two
//! ways clause 6.3.5 allows -- a `Link` header with `application/json`, or an
//! inline `@context` with `application/ld+json` -- and a registered source
//! enforces the same rule on what it receives. [`crate::forwarding_context`]
//! decides once, per registration, which single context a forwarded request is
//! expressed against; this module writes the request that way: which context
//! URL goes into its one `Link` header, and what the body looks like once it is
//! spelled in that context and the registration's `contextSourceInfo` has been
//! applied.

use crate::{
    document_rewording::{DocumentRewording, DocumentVocabulary},
    error::FederationError,
    forwarded_names::ForwardedNames,
    forwarding_context::{ForwardingContext, ForwardingDecision, Rewording},
    term_resolver::TermResolver,
};
use andromeda_http_headers::media_type::ResponseMediaType;
use andromeda_jsonld::{batch_contexts::BatchContext, context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::context_source_info::ContextSourceInfo;
use andromeda_ngsi_ld_render::version::{adapted_payload::AdaptedPayload, renderer::VersionRenderer};
use iri_rs::IriBuf;
use serde_json::Value;

/// The representation one forwarded request is sent in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardedPayload {
    /// The media type of the forwarded body, which also decides how the
    /// context is carried (clause 6.3.5).
    pub media_type: ResponseMediaType,
    /// The context IRI placed in the one `Link` header; absent for
    /// `application/ld+json`, whose context travels inline, and for a request
    /// that names the default context. A forwarded request never names two
    /// (JSON-LD 1.1 section 6.1).
    pub link_context: Option<IriBuf>,
    /// The forwarded body, absent for a body-less request.
    pub body: Option<Vec<u8>>,
}

/// The body one forwarded request carries, and whose terms it is written in.
#[derive(Debug, Clone, Copy)]
pub(crate) enum OutgoingBody<'a> {
    /// A client's document, or its narrowing to one registration, written in
    /// the inbound context's terms.
    Document {
        /// The serialised document.
        bytes: &'a [u8],
        /// What kind of document it is, which decides where its names sit.
        vocabulary: DocumentVocabulary,
        /// Which NGSI-LD Element its Entity data is.
        element: AdaptedPayload,
    },
    /// A reduced subscription (clause 5.8.1.4), written in the inbound
    /// context's terms.
    Subscription(&'a [u8]),
    /// A document this broker wrote in the forwarding context's terms already,
    /// such as the `Query` of a forwarded batch query (clause 6.23).
    Expressed(&'a [u8]),
}

/// How the clause 4.3.6.8 fallback reads one outgoing body.
///
/// Tables 4.3.6.8-1 to -3 describe Entity data, so only a body that carries
/// it is amended, and each by the reading its shape calls for (clause
/// 4.3.6.6): an entity, an entity fragment or an array of them as Entity data;
/// the one attribute a request path names as that attribute's instances.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FallbackReading {
    /// Entities of the given Element.
    Entities(AdaptedPayload),
    /// One attribute of an entity of the given Element.
    Attribute(AdaptedPayload),
    /// No Entity data: a Subscription, a batch `Query`, a discovery answer or
    /// an `EntityMap`.
    Untouched,
}

impl OutgoingBody<'_> {
    /// The reading the fallback takes of the body.
    const fn fallback_reading(self) -> FallbackReading {
        match self {
            OutgoingBody::Document {
                vocabulary: DocumentVocabulary::Entities,
                element,
                ..
            } => FallbackReading::Entities(element),
            OutgoingBody::Document {
                vocabulary: DocumentVocabulary::Attribute,
                element,
                ..
            } => FallbackReading::Attribute(element),
            OutgoingBody::Document {
                vocabulary: DocumentVocabulary::Discovery | DocumentVocabulary::Opaque,
                ..
            }
            | OutgoingBody::Subscription(_)
            | OutgoingBody::Expressed(_) => FallbackReading::Untouched,
        }
    }
}

/// Strip an inline `@context` from every object of a body.
fn remove_inline_context(body: &mut Value) {
    match body {
        Value::Object(object) => {
            object.remove("@context");
        }
        Value::Array(items) => items.iter_mut().for_each(remove_inline_context),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

/// Place `context` as the inline `@context` of every object of a body.
fn embed_inline_context(body: &mut Value, context: &Value) {
    match body {
        Value::Object(object) => {
            object.insert("@context".to_string(), context.clone()); // clone: every object of a batch carries its own copy
        }
        Value::Array(items) => items.iter_mut().for_each(|item| embed_inline_context(item, context)),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

/// Build the payload of one forwarded request.
///
/// `inbound` is the context the triggering request resolved to; `decision` is
/// the registration's forwarding decision; `body` is the request body without
/// its inline `@context`; `info` is the registration's `contextSourceInfo`.
///
/// # Errors
///
/// Returns [`FederationError::Representation`] when the body is not JSON, and
/// [`FederationError::JsonLd`] or [`FederationError::Query`] when it cannot be
/// spelled in the forwarding context.
pub(crate) async fn build_forwarded_payload(
    engine: &JsonLdEngine,
    inbound: &ContextSpec,
    decision: &ForwardingDecision,
    body: Option<OutgoingBody<'_>>,
    info: &ContextSourceInfo<'_>,
) -> Result<ForwardedPayload, FederationError> {
    let media_type = decision.media_type;
    let link_context = decision.context.link();
    let Some(body) = body else {
        return Ok(ForwardedPayload {
            media_type,
            link_context,
            body: None,
        });
    };

    let reading = body.fallback_reading();
    let mut value = conformant(spelled_body(engine, inbound, &decision.context, body).await?, info, reading);
    // Clause 6.3.5: `application/ld+json` carries its context inline and never
    // beside a context `Link`.
    if let Some(context) = decision.context.inline_for(inbound) {
        embed_inline_context(&mut value, &context);
    }
    Ok(ForwardedPayload {
        media_type,
        link_context,
        body: Some(serde_json::to_vec(&value)?),
    })
}

/// One entity of a forwarded batch array, and the context it was submitted
/// under: JSON-LD applies a top-level node's `@context` to that node alone, so
/// each entity of an `application/ld+json` batch is read under its own.
#[derive(Debug)]
pub(crate) struct ItemBody<'a> {
    /// The entity.
    pub(crate) body: Value,
    /// The context the entity was submitted under.
    pub(crate) context: &'a BatchContext,
}

/// Build the payload of one forwarded batch array, each entity spelled from
/// its own context into the one `decision` names.
///
/// # Errors
///
/// Returns [`FederationError::JsonLd`] when an entity cannot be spelled in the
/// forwarding context, and [`FederationError::Representation`] when the array
/// cannot be serialised.
pub(crate) async fn build_items_payload(
    engine: &JsonLdEngine,
    decision: &ForwardingDecision,
    items: Vec<ItemBody<'_>>,
    info: &ContextSourceInfo<'_>,
) -> Result<ForwardedPayload, FederationError> {
    let mut array = Vec::with_capacity(items.len());
    for ItemBody { body: mut value, context } in items {
        remove_inline_context(&mut value);
        let naming = decision.context.naming(context.spec());
        let mut value = DocumentRewording::reword(
            engine,
            context.spec(),
            &naming,
            value,
            DocumentVocabulary::Entities,
            decision.context.rewording(),
        )
        .await?;
        value = conformant(value, info, FallbackReading::Entities(AdaptedPayload::Entity));
        if let Some(inline) = decision.context.inline_for(context.spec()) {
            embed_inline_context(&mut value, &inline);
        }
        array.push(value);
    }
    Ok(ForwardedPayload {
        media_type: decision.media_type,
        link_context: decision.context.link(),
        body: Some(serde_json::to_vec(&Value::Array(array))?),
    })
}

/// Build the payload of a forwarded batch deletion: the bare Entity IDs, as
/// `application/json` under no context `Link`.
///
/// Clause 6.3.5 has an `application/ld+json` body name its context in an
/// `@context` member, which an array of strings cannot carry, and the
/// identifiers name no term a context could resolve, so the array is sent the
/// one way a source admits it whatever representation its registration asks
/// for.
///
/// # Errors
///
/// Returns [`FederationError::Representation`] when the array cannot be
/// serialised.
pub(crate) fn build_identifiers_payload(ids: &[Identifier]) -> Result<ForwardedPayload, FederationError> {
    Ok(ForwardedPayload {
        media_type: ResponseMediaType::Json,
        link_context: None,
        body: Some(serde_json::to_vec(ids)?),
    })
}

/// The payload conforming to the version the registration's
/// `ngsildConformance` says its source understands (clauses 4.3.6.6 and
/// 4.3.6.8), read as `reading` says, or unchanged when it names none.
fn conformant(value: Value, info: &ContextSourceInfo<'_>, reading: FallbackReading) -> Value {
    let Some(version) = info.ngsild_conformance else {
        return value;
    };
    match reading {
        FallbackReading::Entities(element) => VersionRenderer::new(version).with_adapted_payload(element).with_payload(value).render().0,
        FallbackReading::Attribute(element) => {
            VersionRenderer::new(version)
                .with_adapted_payload(element)
                .with_payload(value)
                .render_attribute()
                .0
        }
        FallbackReading::Untouched => value,
    }
}

/// The body spelled in the forwarding context, without an inline `@context`.
async fn spelled_body(engine: &JsonLdEngine, inbound: &ContextSpec, context: &ForwardingContext, body: OutgoingBody<'_>) -> Result<Value, FederationError> {
    let (OutgoingBody::Document { bytes, .. } | OutgoingBody::Subscription(bytes) | OutgoingBody::Expressed(bytes)) = body;
    let mut value: Value = serde_json::from_slice(bytes)?;
    remove_inline_context(&mut value);
    match (context.rewording(), body) {
        (Rewording::AsInbound, _) | (Rewording::Reword, OutgoingBody::Expressed(_)) => Ok(value),
        (Rewording::Reword, OutgoingBody::Document { vocabulary, .. }) => {
            // Clause 4.3.6.6: the document is spelled in the forwarding
            // context, and carries no `@context` member of its own.
            DocumentRewording::reword(engine, inbound, &context.naming(inbound), value, vocabulary, Rewording::Reword).await
        }
        (Rewording::Reword, OutgoingBody::Subscription(_)) => {
            let naming = context.naming(inbound);
            let resolver = TermResolver::new(engine, inbound);
            ForwardedNames::prepare(engine, &resolver, &naming, Rewording::Reword)
                .await?
                .subscription_document(value)
                .await
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        document_rewording::DocumentVocabulary,
        forwarded_payload::{ForwardedPayload, ItemBody, OutgoingBody, build_forwarded_payload, build_identifiers_payload, build_items_payload},
        forwarding_context::{BodyPresence, ForwardingContext, Rewording, decide, decide_for_items},
    };
    use andromeda_http_headers::media_type::ResponseMediaType;
    use andromeda_jsonld::{batch_contexts::BatchContexts, context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::{context_source_info::ContextSourceInfo, key_value_pair::KeyValuePair};
    use andromeda_ngsi_ld_render::version::adapted_payload::AdaptedPayload;
    use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
    use iri_rs::IriBuf;
    use serde_json::{Value, json};
    use std::{
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

    fn body() -> Vec<u8> {
        serde_json::to_vec(&json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "speed": {"type": "Property", "value": 10}})).expect("serialises")
    }

    /// An entity document written in the inbound context's terms.
    const fn document(bytes: &[u8]) -> OutgoingBody<'_> {
        OutgoingBody::Document {
            bytes,
            vocabulary: DocumentVocabulary::Entities,
            element: AdaptedPayload::Entity,
        }
    }

    fn body_context(payload_body: &[u8]) -> Option<Value> {
        serde_json::from_slice::<Value>(payload_body).expect("JSON body").get("@context").cloned()
    }

    async fn payload(inbound: &ContextSpec, entries: &[(&str, &str)], body: Option<OutgoingBody<'_>>) -> ForwardedPayload {
        let raw = pairs(entries);
        let info = ContextSourceInfo::try_from(raw.as_slice()).expect("valid info");
        let presence = if body.is_some() { BodyPresence::Present } else { BodyPresence::Absent };
        let decision = decide(inbound, &info, None, presence).expect("decides");
        build_forwarded_payload(&engine(), inbound, &decision, body, &info)
            .await
            .expect("payload builds")
    }

    #[tokio::test]
    async fn a_link_context_is_forwarded_as_application_json_with_the_same_link_and_no_inline_context() {
        let bytes = body();
        let payload = payload(&link_spec(USER_CONTEXT), &[], Some(document(&bytes))).await;
        pretty_assertions::assert_eq!(payload.media_type, ResponseMediaType::Json);
        pretty_assertions::assert_eq!(payload.link_context.as_ref().map(IriBuf::as_str), Some(USER_CONTEXT));
        let forwarded: Value = serde_json::from_slice(payload.body.as_deref().expect("body")).expect("JSON");
        assert!(forwarded.get("@context").is_none());
        assert!(forwarded.get("speed").is_some(), "the client's terms travel unchanged: {forwarded}");
    }

    #[tokio::test]
    async fn a_content_type_of_ld_json_inlines_the_context_and_sends_no_link() {
        let bytes = body();
        let payload = payload(&link_spec(USER_CONTEXT), &[("contentType", "application/ld+json")], Some(document(&bytes))).await;
        pretty_assertions::assert_eq!(payload.media_type, ResponseMediaType::JsonLd);
        assert!(payload.link_context.is_none());
        pretty_assertions::assert_eq!(body_context(payload.body.as_deref().expect("body")), Some(json!(USER_CONTEXT)));
    }

    #[tokio::test]
    async fn an_inline_url_context_travels_as_a_link_under_application_json() {
        let bytes = body();
        let payload = payload(&ContextSpec::from_body(json!(USER_CONTEXT)), &[], Some(document(&bytes))).await;
        pretty_assertions::assert_eq!(payload.media_type, ResponseMediaType::Json);
        pretty_assertions::assert_eq!(payload.link_context.as_ref().map(IriBuf::as_str), Some(USER_CONTEXT));
        assert!(body_context(payload.body.as_deref().expect("body")).is_none());
    }

    /// JSON-LD 1.1 section 6.1: several inbound context URLs are never
    /// forwarded as several context links. Under `application/json` the body
    /// is compacted against the one core context link, which writes a user term
    /// as its full IRI; under `application/ld+json` the URLs travel inline.
    #[tokio::test]
    async fn several_inbound_urls_are_forwarded_under_one_core_link_with_full_names() {
        let bytes = body();
        let inbound = ContextSpec::from_body(json!([USER_CONTEXT, OTHER_CONTEXT]));
        let as_json = payload(&inbound, &[], Some(document(&bytes))).await;
        pretty_assertions::assert_eq!(as_json.link_context.as_ref().map(IriBuf::as_str), Some(NGSI_LD_CORE_CONTEXT_URL.as_str()));
        let forwarded: Value = serde_json::from_slice(as_json.body.as_deref().expect("body")).expect("JSON");
        assert!(forwarded.get("@context").is_none());
        pretty_assertions::assert_eq!(forwarded["type"], json!("https://example.org/Vehicle"));
        pretty_assertions::assert_eq!(forwarded["https://example.org/speed"]["value"], json!(10));
        pretty_assertions::assert_eq!(forwarded["id"], json!("urn:ngsi-ld:Vehicle:1"));

        let as_ld = payload(&inbound, &[("contentType", "application/ld+json")], Some(document(&bytes))).await;
        assert!(as_ld.link_context.is_none());
        pretty_assertions::assert_eq!(body_context(as_ld.body.as_deref().expect("body")), Some(json!([USER_CONTEXT, OTHER_CONTEXT])));
    }

    #[tokio::test]
    async fn an_inline_object_context_is_re_inlined_for_ld_json_and_compacted_under_the_core_link_for_json() {
        let bytes = body();
        let inline = json!({"speed": "https://example.org/speed", "Vehicle": "https://example.org/Vehicle"});
        let inbound = ContextSpec::from_body(inline.clone()); // clone: compared below

        let as_ld = payload(&inbound, &[("contentType", "application/ld+json")], Some(document(&bytes))).await;
        pretty_assertions::assert_eq!(as_ld.media_type, ResponseMediaType::JsonLd);
        assert!(as_ld.link_context.is_none());
        pretty_assertions::assert_eq!(body_context(as_ld.body.as_deref().expect("body")), Some(inline));

        let as_json = payload(&inbound, &[], Some(document(&bytes))).await;
        pretty_assertions::assert_eq!(as_json.link_context.as_ref().map(IriBuf::as_str), Some(NGSI_LD_CORE_CONTEXT_URL.as_str()));
        let forwarded: Value = serde_json::from_slice(as_json.body.as_deref().expect("body")).expect("JSON body");
        pretty_assertions::assert_eq!(forwarded["https://example.org/speed"]["type"], json!("Property"));
        assert!(forwarded.get("@context").is_none());
    }

    /// Clause 4.3.6.6: a configured `jsonldContext` forces `application/json`,
    /// names that context in the one `Link`, and the body is compacted against
    /// it -- `speed` becomes `velocity` and `Vehicle` becomes `Car`, which only
    /// the configured context spells that way.
    #[tokio::test]
    async fn a_configured_jsonld_context_forces_application_json_and_compacts_against_it() {
        let bytes = body();
        let payload = payload(
            &link_spec(USER_CONTEXT),
            &[("jsonldContext", CONFIGURED_CONTEXT), ("contentType", "application/ld+json")],
            Some(document(&bytes)),
        )
        .await;
        pretty_assertions::assert_eq!(payload.media_type, ResponseMediaType::Json);
        pretty_assertions::assert_eq!(payload.link_context.as_ref().map(IriBuf::as_str), Some(CONFIGURED_CONTEXT));
        let forwarded: Value = serde_json::from_slice(payload.body.as_deref().expect("body")).expect("JSON body");
        assert!(forwarded.get("@context").is_none());
        pretty_assertions::assert_eq!(forwarded["type"], json!("Car"));
        pretty_assertions::assert_eq!(forwarded["velocity"]["value"], json!(10));
        assert!(forwarded.get("speed").is_none(), "{forwarded}");
    }

    /// A body this broker already wrote in the forwarding context is sent as
    /// written.
    #[tokio::test]
    async fn an_expressed_body_is_never_respelled() {
        let query = serde_json::to_vec(&json!({"type": "Query", "entities": [{"type": "Car"}], "attrs": ["velocity"]})).expect("serialises");
        let payload = payload(
            &link_spec(USER_CONTEXT),
            &[("jsonldContext", CONFIGURED_CONTEXT)],
            Some(OutgoingBody::Expressed(&query)),
        )
        .await;
        let forwarded: Value = serde_json::from_slice(payload.body.as_deref().expect("body")).expect("JSON body");
        pretty_assertions::assert_eq!(forwarded, json!({"type": "Query", "entities": [{"type": "Car"}], "attrs": ["velocity"]}));
    }

    /// A reduced subscription is respelled member by member rather than
    /// through a JSON-LD round trip, so its shape survives.
    #[tokio::test]
    async fn a_subscription_body_is_respelled_member_by_member() {
        let subscription = serde_json::to_vec(&json!({
            "id": "urn:ngsi-ld:Subscription:forwarded-1",
            "type": "Subscription",
            "entities": [{"type": "Vehicle"}],
            "watchedAttributes": ["speed"],
            "q": "speed>1",
            "notification": {"endpoint": {"uri": "http://receiver.example.org/"}}
        }))
        .expect("serialises");
        let payload = payload(
            &link_spec(USER_CONTEXT),
            &[("jsonldContext", CONFIGURED_CONTEXT)],
            Some(OutgoingBody::Subscription(&subscription)),
        )
        .await;
        let forwarded: Value = serde_json::from_slice(payload.body.as_deref().expect("body")).expect("JSON body");
        pretty_assertions::assert_eq!(forwarded["entities"], json!([{"type": "Car"}]));
        pretty_assertions::assert_eq!(forwarded["watchedAttributes"], json!(["velocity"]));
        pretty_assertions::assert_eq!(forwarded["q"], json!("velocity>1"));
    }

    /// Review change 17: a relative reference is never forwarded. An inline
    /// context holding one -- as an entry, or as an `@import` -- would resolve
    /// against the source's URL rather than this broker's request, so the
    /// `application/ld+json` body is written inline against the core context
    /// instead, every user term as its full IRI.
    #[tokio::test]
    async fn an_inline_context_holding_a_relative_reference_is_never_forwarded() {
        let query = serde_json::to_vec(&json!({"type": "Query", "entities": [{"type": "https://example.org/Vehicle"}]})).expect("serialises");
        let ld = [("contentType", "application/ld+json")];
        for relative in [
            json!(["ctx.jsonld", USER_CONTEXT]),
            json!("/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:JsonLdContext:1"),
            json!([USER_CONTEXT, {"@import": "shared/terms.jsonld", "brand": "https://example.org/brand"}]),
        ] {
            let inbound = ContextSpec::from_body(relative);
            let raw = pairs(&ld);
            let info = ContextSourceInfo::try_from(raw.as_slice()).expect("valid info");
            let decision = decide(&inbound, &info, None, BodyPresence::Present).expect("decides");
            pretty_assertions::assert_eq!(decision.context, ForwardingContext::InlineCore);
            pretty_assertions::assert_eq!(decision.context.rewording(), Rewording::Reword);
            pretty_assertions::assert_eq!(decision.context.naming(&inbound), ContextSpec::none());

            let forwarded = payload(&inbound, &ld, Some(OutgoingBody::Expressed(&query))).await;
            pretty_assertions::assert_eq!(forwarded.media_type, ResponseMediaType::JsonLd);
            assert!(forwarded.link_context.is_none());
            pretty_assertions::assert_eq!(
                body_context(forwarded.body.as_deref().expect("body")),
                Some(json!(NGSI_LD_CORE_CONTEXT_URL.as_str()))
            );
        }
    }

    /// An inline context whose every reference is absolute travels as written,
    /// including an absolute `@import`.
    #[tokio::test]
    async fn an_inline_context_of_absolute_references_travels_as_written() {
        let bytes = body();
        let absolute = json!([USER_CONTEXT, {"@import": OTHER_CONTEXT, "colour": "https://example.org/colour"}]);
        let inbound = ContextSpec::from_body(absolute.clone()); // clone: compared below
        let forwarded = payload(&inbound, &[("contentType", "application/ld+json")], Some(document(&bytes))).await;
        pretty_assertions::assert_eq!(body_context(forwarded.body.as_deref().expect("body")), Some(absolute));
    }

    /// The batch payload of one registration, each item under its own context.
    async fn items_payload(entries: &[(&str, &str)], items: Vec<ItemBody<'_>>) -> ForwardedPayload {
        let raw = pairs(entries);
        let info = ContextSourceInfo::try_from(raw.as_slice()).expect("valid info");
        let decision = decide_for_items(items.iter().map(|item| item.context), &info, None).expect("decides");
        build_items_payload(&engine(), &decision, items, &info).await.expect("payload builds")
    }

    /// Items submitted under different contexts have no single context a `Link`
    /// can name: under `application/json` the batch names the core context
    /// once, and each item is spelled from its own context as full IRIs.
    #[tokio::test]
    async fn items_under_different_contexts_are_forwarded_under_one_core_link_each_from_its_own_context() {
        let mut contexts = BatchContexts::per_entity();
        let user = contexts.context_of(Some(json!(USER_CONTEXT)));
        let configured = contexts.context_of(Some(json!(CONFIGURED_CONTEXT)));
        let items = vec![
            ItemBody {
                body: json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "speed": {"type": "Property", "value": 1}}),
                context: &user,
            },
            ItemBody {
                body: json!({"id": "urn:ngsi-ld:Vehicle:2", "type": "Car", "velocity": {"type": "Property", "value": 2}}),
                context: &configured,
            },
        ];
        let forwarded = items_payload(&[], items).await;
        pretty_assertions::assert_eq!(forwarded.media_type, ResponseMediaType::Json);
        pretty_assertions::assert_eq!(forwarded.link_context.as_ref().map(IriBuf::as_str), Some(NGSI_LD_CORE_CONTEXT_URL.as_str()));
        let body: Value = serde_json::from_slice(forwarded.body.as_deref().expect("body")).expect("JSON body");
        for (index, value) in [(0, 1), (1, 2)] {
            pretty_assertions::assert_eq!(body[index]["type"], json!("https://example.org/Vehicle"), "{body}");
            pretty_assertions::assert_eq!(body[index]["https://example.org/speed"]["value"], json!(value), "{body}");
            assert!(body[index].get("@context").is_none(), "{body}");
        }
    }

    /// Under `application/ld+json` each item carries its own context inline,
    /// and the batch names no context `Link` (clause 6.3.5).
    #[tokio::test]
    async fn items_under_different_contexts_carry_each_its_own_inline_context_under_ld_json() {
        let mut contexts = BatchContexts::per_entity();
        let user = contexts.context_of(Some(json!(USER_CONTEXT)));
        let configured = contexts.context_of(Some(json!(CONFIGURED_CONTEXT)));
        let items = vec![
            ItemBody {
                body: json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle"}),
                context: &user,
            },
            ItemBody {
                body: json!({"id": "urn:ngsi-ld:Vehicle:2", "type": "Car"}),
                context: &configured,
            },
        ];
        let forwarded = items_payload(&[("contentType", "application/ld+json")], items).await;
        assert!(forwarded.link_context.is_none());
        let body: Value = serde_json::from_slice(forwarded.body.as_deref().expect("body")).expect("JSON body");
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
        let body: Value = serde_json::from_slice(forwarded.body.as_deref().expect("body")).expect("JSON body");
        pretty_assertions::assert_eq!(body, json!(["urn:ngsi-ld:Vehicle:1", "urn:ngsi-ld:Vehicle:2"]));
    }

    /// An item whose own context holds a relative reference cannot travel
    /// inline, so the whole batch is written inline against the core context.
    #[test]
    fn items_under_ld_json_with_a_relative_reference_fall_back_to_the_inline_core_context() {
        let mut contexts = BatchContexts::per_entity();
        let user = contexts.context_of(Some(json!(USER_CONTEXT)));
        let relative = contexts.context_of(Some(json!(["relative.jsonld", USER_CONTEXT])));
        let other = contexts.context_of(Some(json!(OTHER_CONTEXT)));
        let raw = pairs(&[("contentType", "application/ld+json")]);
        let info = ContextSourceInfo::try_from(raw.as_slice()).expect("valid info");

        let decision = decide_for_items([&user, &relative], &info, None).expect("decides");
        pretty_assertions::assert_eq!(decision.context, ForwardingContext::InlineCore);
        pretty_assertions::assert_eq!(decision.context.inline_for(user.spec()), Some(json!(NGSI_LD_CORE_CONTEXT_URL.as_str())));
        pretty_assertions::assert_eq!(decision.context.naming(relative.spec()), ContextSpec::none());

        let absolute_only = decide_for_items([&user, &other], &info, None).expect("decides");
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

    #[tokio::test]
    async fn an_ngsild_conformance_applies_the_clause_4_3_6_8_fallbacks() {
        let versioned = serde_json::to_vec(&json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "scope": "/Madrid",
            "speed": {"type": "Property", "value": 10, "observedAt": "2024-01-01T00:00:00Z"}
        }))
        .expect("serialises");
        let payload = payload(&link_spec(USER_CONTEXT), &[("ngsildConformance", "1.0")], Some(document(&versioned))).await;
        let forwarded: Value = serde_json::from_slice(payload.body.as_deref().expect("body")).expect("JSON body");
        assert!(forwarded.get("scope").is_none(), "scope was introduced in 1.4: {forwarded}");
        assert!(forwarded["speed"].get("observedAt").is_none(), "observedAt was introduced in 1.3: {forwarded}");
    }
    /// The body forwarded to a 1.0 source for `body`.
    async fn forwarded_to_1_0(body: OutgoingBody<'_>) -> Value {
        let payload = payload(&link_spec(USER_CONTEXT), &[("ngsildConformance", "1.0")], Some(body)).await;
        serde_json::from_slice(payload.body.as_deref().expect("body")).expect("JSON body")
    }

    /// Tables 4.3.6.8-x describe Entity data alone: a forwarded Subscription
    /// keeps its own `expiresAt` and every `entities` selector.
    #[tokio::test]
    async fn a_forwarded_subscription_is_not_read_as_an_entity() {
        let subscription = json!({
            "id": "urn:ngsi-ld:Subscription:forwarded-1",
            "type": "Subscription",
            "entities": [{"type": "Vehicle"}, {"type": "Bus"}],
            "expiresAt": "2030-01-01T00:00:00Z",
            "notification": {"endpoint": {"uri": "http://receiver.example.org/"}}
        });
        let bytes = serde_json::to_vec(&subscription).expect("serialises");
        let forwarded = forwarded_to_1_0(OutgoingBody::Subscription(&bytes)).await;
        pretty_assertions::assert_eq!(forwarded["expiresAt"], json!("2030-01-01T00:00:00Z"));
        pretty_assertions::assert_eq!(forwarded["entities"], json!([{"type": "Vehicle"}, {"type": "Bus"}]));
    }

    /// A batch `Query` is no Entity data either.
    #[tokio::test]
    async fn a_forwarded_query_keeps_its_entity_selectors() {
        let query = json!({"type": "Query", "entities": [{"type": "Vehicle"}, {"type": "Bus"}]});
        let bytes = serde_json::to_vec(&query).expect("serialises");
        pretty_assertions::assert_eq!(forwarded_to_1_0(OutgoingBody::Expressed(&bytes)).await, query);
    }

    /// Tables 4.3.6.8-2 and -3 reach the instance members of the one attribute
    /// a request path names.
    #[tokio::test]
    async fn a_forwarded_attribute_body_drops_its_newer_members() {
        let attribute = serde_json::to_vec(&json!({
            "type": "Property",
            "value": 10,
            "observedAt": "2025-01-01T00:00:00Z",
            "unitCode": "KMH",
            "valueType": "Speed"
        }))
        .expect("serialises");
        let forwarded = forwarded_to_1_0(OutgoingBody::Document {
            bytes: &attribute,
            vocabulary: DocumentVocabulary::Attribute,
            element: AdaptedPayload::Entity,
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
        let temporal = serde_json::to_vec(&json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "speed": speeds})).expect("serialises");
        let forwarded = forwarded_to_1_0(OutgoingBody::Document {
            bytes: &temporal,
            vocabulary: DocumentVocabulary::Entities,
            element: AdaptedPayload::TemporalEvolution,
        })
        .await;
        pretty_assertions::assert_eq!(forwarded["speed"], speeds);
    }

    /// Entity data is amended: `scope` arrived at 1.4 and the
    /// `LanguageProperty` at 1.4.
    #[tokio::test]
    async fn a_forwarded_entity_is_amended() {
        let entity = serde_json::to_vec(&json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "scope": "/Madrid",
            "name": {"type": "LanguageProperty", "languageMap": {"en": "bus"}}
        }))
        .expect("serialises");
        let forwarded = forwarded_to_1_0(document(&entity)).await;
        assert!(forwarded.get("scope").is_none(), "{forwarded}");
        pretty_assertions::assert_eq!(forwarded["name"], json!({"type": "Property", "value": {"en": "bus"}}));
    }

    /// The one attribute a request path names is spelled as an attribute in
    /// the forwarding context: its core members stay, what it names moves.
    #[tokio::test]
    async fn an_attribute_body_is_spelled_as_an_attribute_in_the_configured_context() {
        let attribute = serde_json::to_vec(&json!({"type": "Property", "value": 10, "speed": {"type": "Property", "value": 1}})).expect("serialises");
        let payload = payload(
            &link_spec(USER_CONTEXT),
            &[("jsonldContext", CONFIGURED_CONTEXT)],
            Some(OutgoingBody::Document {
                bytes: &attribute,
                vocabulary: DocumentVocabulary::Attribute,
                element: AdaptedPayload::Entity,
            }),
        )
        .await;
        let forwarded: Value = serde_json::from_slice(payload.body.as_deref().expect("body")).expect("JSON body");
        pretty_assertions::assert_eq!(
            forwarded,
            json!({"type": "Property", "value": 10, "velocity": {"type": "Property", "value": 1}})
        );
    }
}
