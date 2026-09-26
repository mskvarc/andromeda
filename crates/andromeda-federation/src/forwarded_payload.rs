//! The representation a forwarded request carries (clauses 4.3.6.6, 4.3.6.8, 6.3.5, 6.3.19).
//!
//! A request arriving at this broker carries its `@context` in one of the two
//! ways clause 6.3.5 allows — `Link` headers with `application/json`, or an
//! inline `@context` with `application/ld+json` — and a registered source
//! enforces the same rule on what it receives. Mixing them, as forwarding an
//! `application/ld+json` body alongside a context `Link`, is a
//! `BadRequestData` at the receiver. This module decides, once, how one
//! forwarded request carries its context: which media type it is sent as, which
//! context URLs go into `Link` headers, and what the body looks like after the
//! registration's `contextSourceInfo` has been applied.

use crate::error::FederationError;
use andromeda_http_headers::media_type::ResponseMediaType;
use andromeda_jsonld::{
    context_spec::ContextSpec,
    engine::{options::CompactOptions, processor::JsonLdEngine},
};
use andromeda_ngsi_ld_model::context_source_info::ContextSourceInfo;
use andromeda_ngsi_ld_render::version::renderer::VersionRenderer;
use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
use iri_rs::IriRefBuf;
use serde_json::Value;
use url::Url;

/// The representation one forwarded request is sent in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardedPayload {
    /// The media type of the forwarded body, which also decides how the
    /// context is carried (clause 6.3.5).
    pub media_type: ResponseMediaType,
    /// Context URLs placed in `Link` headers; empty for `application/ld+json`,
    /// whose context travels inline, and for a request that names none.
    pub link_contexts: Vec<IriRefBuf>,
    /// The forwarded body, absent for a body-less request.
    pub body: Option<Vec<u8>>,
}

/// The context a forwarded request is expressed against.
enum TargetContext {
    /// The registration's `jsonldContext`: compaction against it is mandatory.
    Configured(IriRefBuf),
    /// The inbound request's own context, a list of URLs.
    InboundUrls(Vec<IriRefBuf>),
    /// The inbound request's own context, an inline object or an array that
    /// carries one, which no `Link` header can name.
    InboundInline(Value),
}

/// Whether the `@context` a request carried can be named in a `Link` header.
fn as_context_urls(context: &Value) -> Option<Vec<IriRefBuf>> {
    match context {
        Value::String(url) => IriRefBuf::new(url.clone()).ok().map(|url| vec![url]), // clone: the header owns its IRI
        Value::Array(items) => items
            .iter()
            .map(|item| item.as_str().and_then(|url| IriRefBuf::new(url.to_string()).ok()))
            .collect(),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_) => None,
    }
}

/// Decide which context the forwarded request is expressed against.
fn target_context(inbound: &ContextSpec, configured: Option<&IriRefBuf>) -> TargetContext {
    if let Some(configured) = configured {
        return TargetContext::Configured(configured.clone()); // clone: the payload owns the configured IRI
    }
    let mut urls = inbound.link_contexts().to_vec(); // clone: the payload owns its Link IRIs
    match inbound.body_context() {
        Some(inline) => match as_context_urls(inline) {
            Some(inline_urls) => {
                urls.extend(inline_urls);
                TargetContext::InboundUrls(urls)
            }
            None => TargetContext::InboundInline(inline.clone()), // clone: the inline context is re-embedded or expanded against
        },
        None => TargetContext::InboundUrls(urls),
    }
}

/// The context URL reference of the core context.
fn core_context_url() -> IriRefBuf {
    IriRefBuf::from(NGSI_LD_CORE_CONTEXT_URL.as_iri_ref().to_owned())
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
/// `inbound` is the context the triggering request resolved to; `body` is the
/// request body without its inline `@context`; `info` is the registration's
/// `contextSourceInfo`; `configured_context` is the registration's
/// `jsonldContext`, whether it came from `contextSourceInfo` or from the
/// registration member of the same name.
pub(crate) async fn build_forwarded_payload(
    engine: &JsonLdEngine,
    inbound: &ContextSpec,
    body: Option<&[u8]>,
    info: &ContextSourceInfo<'_>,
    configured_context: Option<&Url>,
) -> Result<ForwardedPayload, FederationError> {
    let configured = match (&info.jsonld_context, configured_context) {
        (Some(from_info), _) => Some(from_info.clone()), // clone: the payload owns the IRI
        (None, Some(member)) => Some(
            IriRefBuf::new(member.as_str().to_string()).map_err(|error| FederationError::internal(format!("invalid registration jsonldContext: {error}")))?,
        ),
        (None, None) => None,
    };
    let target = target_context(inbound, configured.as_ref());
    // Clause 6.3.19: a configured `jsonldContext` forces `application/json`
    // whenever a body is present; otherwise `contentType` decides and the
    // default is `application/json`, the one form that can name any URL
    // context in a `Link` header (clause 6.3.5).
    let media_type = match (&target, &info.content_type) {
        (TargetContext::Configured(_), Some(_) | None) | (TargetContext::InboundUrls(_) | TargetContext::InboundInline(_), None) => ResponseMediaType::Json,
        (TargetContext::InboundUrls(_) | TargetContext::InboundInline(_), Some(content_type)) => {
            let parsed: mediatype::MediaType<'_> = content_type.into();
            ResponseMediaType::from_media_type(&parsed).unwrap_or_default()
        }
    };

    let Some(body) = body else {
        let link_contexts = match target {
            TargetContext::Configured(url) => vec![url],
            TargetContext::InboundUrls(urls) => urls,
            // A body-less request cannot carry an inline context; the peer
            // expands against the core context, which is what the inline
            // context resolved to for the inbound request's own terms only.
            TargetContext::InboundInline(_) => vec![core_context_url()],
        };
        return Ok(ForwardedPayload {
            media_type,
            link_contexts,
            body: None,
        });
    };

    let mut value: Value = serde_json::from_slice(body)?;
    remove_inline_context(&mut value);

    let (link_contexts, mut value) = match target {
        TargetContext::Configured(url) => {
            // Clause 4.3.6.6: compaction against the configured context is
            // mandatory, and the forwarded body carries no `@context` member.
            let expanded = engine.expand(value, inbound).await?;
            let configured = ContextSpec::from_links(vec![url.clone()]); // clone: the Link header names the same IRI
            let compacted = engine.compactor(&configured).compact(expanded, CompactOptions::default()).await?;
            (vec![url], compacted)
        }
        TargetContext::InboundUrls(urls) => (urls, value),
        TargetContext::InboundInline(inline) => match media_type {
            // The inline context travels with the body, as it arrived.
            ResponseMediaType::JsonLd => (Vec::new(), {
                embed_inline_context(&mut value, &inline);
                value
            }),
            // No `Link` header can name an inline context, so the body is
            // forwarded in expanded form: full IRIs need no context beyond the
            // core one to be read back.
            ResponseMediaType::Json | ResponseMediaType::GeoJson => {
                let expanded = engine.expand(value, inbound).await?;
                (vec![core_context_url()], expanded)
            }
        },
    };

    if let Some(version) = info.ngsild_conformance {
        // Clause 4.3.6.6 `ngsildConformance`: the forwarded payload conforms
        // to the version the source understands (clause 4.3.6.8).
        value = VersionRenderer::new(version).with_payload(value).render().0;
    }

    let (link_contexts, value) = match media_type {
        ResponseMediaType::JsonLd => {
            // Clause 6.3.5: `application/ld+json` carries its context inline
            // and never beside a context `Link`.
            if !link_contexts.is_empty() {
                let inline = match link_contexts.as_slice() {
                    [single] => Value::String(single.to_string()),
                    many => Value::Array(many.iter().map(|url| Value::String(url.to_string())).collect()),
                };
                embed_inline_context(&mut value, &inline);
            } else if value.get("@context").is_none() && value.is_object() {
                embed_inline_context(&mut value, &Value::String(NGSI_LD_CORE_CONTEXT_URL.to_string()));
            }
            (Vec::new(), value)
        }
        ResponseMediaType::Json | ResponseMediaType::GeoJson => (link_contexts, value),
    };

    Ok(ForwardedPayload {
        media_type,
        link_contexts,
        body: Some(serde_json::to_vec(&value)?),
    })
}

#[cfg(test)]
mod tests {
    use crate::forwarded_payload::build_forwarded_payload;
    use andromeda_http_headers::media_type::ResponseMediaType;
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_model::{context_source_info::ContextSourceInfo, key_value_pair::KeyValuePair};
    use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
    use iri_rs::IriRefBuf;
    use serde_json::{Value, json};

    const USER_CONTEXT: &str = "https://example.org/user-context.jsonld";

    fn engine() -> JsonLdEngine {
        JsonLdEngine::builder().allow_remote(false).build().expect("offline engine")
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

    fn link_spec(urls: &[&str]) -> ContextSpec {
        ContextSpec::from_links(urls.iter().map(|url| IriRefBuf::new((*url).to_string()).expect("valid IRI")).collect())
    }

    fn body() -> Vec<u8> {
        serde_json::to_vec(&json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "speed": {"type": "Property", "value": 10}})).expect("serialises")
    }

    fn body_context(payload_body: &[u8]) -> Option<Value> {
        serde_json::from_slice::<Value>(payload_body).expect("JSON body").get("@context").cloned()
    }

    #[tokio::test]
    async fn a_link_context_is_forwarded_as_application_json_with_the_same_link_and_no_inline_context() {
        let info_pairs = pairs(&[]);
        let info = ContextSourceInfo::try_from(info_pairs.as_slice()).expect("valid info");
        let payload = build_forwarded_payload(&engine(), &link_spec(&[USER_CONTEXT]), Some(&body()), &info, None)
            .await
            .expect("payload builds");
        pretty_assertions::assert_eq!(payload.media_type, ResponseMediaType::Json);
        pretty_assertions::assert_eq!(
            payload.link_contexts.iter().map(ToString::to_string).collect::<Vec<_>>(),
            vec![USER_CONTEXT.to_string()]
        );
        assert!(body_context(payload.body.as_deref().expect("body")).is_none());
    }

    #[tokio::test]
    async fn a_content_type_of_ld_json_inlines_the_context_and_sends_no_link() {
        let info_pairs = pairs(&[("contentType", "application/ld+json")]);
        let info = ContextSourceInfo::try_from(info_pairs.as_slice()).expect("valid info");
        let payload = build_forwarded_payload(&engine(), &link_spec(&[USER_CONTEXT]), Some(&body()), &info, None)
            .await
            .expect("payload builds");
        pretty_assertions::assert_eq!(payload.media_type, ResponseMediaType::JsonLd);
        assert_eq!(payload.link_contexts, [] as [iri_rs::IriRef<String>; 0]);
        pretty_assertions::assert_eq!(body_context(payload.body.as_deref().expect("body")), Some(json!(USER_CONTEXT)));
    }

    #[tokio::test]
    async fn an_inline_url_context_travels_as_a_link_under_application_json() {
        let info_pairs = pairs(&[]);
        let info = ContextSourceInfo::try_from(info_pairs.as_slice()).expect("valid info");
        let inbound = ContextSpec::none().with_body_context(Some(json!(USER_CONTEXT)));
        let payload = build_forwarded_payload(&engine(), &inbound, Some(&body()), &info, None)
            .await
            .expect("payload builds");
        pretty_assertions::assert_eq!(payload.media_type, ResponseMediaType::Json);
        pretty_assertions::assert_eq!(
            payload.link_contexts.iter().map(ToString::to_string).collect::<Vec<_>>(),
            vec![USER_CONTEXT.to_string()]
        );
        assert!(body_context(payload.body.as_deref().expect("body")).is_none());
    }

    #[tokio::test]
    async fn an_inline_object_context_is_re_inlined_for_ld_json_and_expanded_under_the_core_link_for_json() {
        let inline = json!({"speed": "https://example.org/speed"});
        let inbound = ContextSpec::none().with_body_context(Some(inline.clone())); // clone: compared below

        let ld_pairs = pairs(&[("contentType", "application/ld+json")]);
        let ld_info = ContextSourceInfo::try_from(ld_pairs.as_slice()).expect("valid info");
        let as_ld = build_forwarded_payload(&engine(), &inbound, Some(&body()), &ld_info, None)
            .await
            .expect("payload builds");
        pretty_assertions::assert_eq!(as_ld.media_type, ResponseMediaType::JsonLd);
        assert_eq!(as_ld.link_contexts, [] as [iri_rs::IriRef<String>; 0]);
        pretty_assertions::assert_eq!(body_context(as_ld.body.as_deref().expect("body")), Some(inline));

        let json_pairs = pairs(&[]);
        let json_info = ContextSourceInfo::try_from(json_pairs.as_slice()).expect("valid info");
        let as_json = build_forwarded_payload(&engine(), &inbound, Some(&body()), &json_info, None)
            .await
            .expect("payload builds");
        pretty_assertions::assert_eq!(as_json.media_type, ResponseMediaType::Json);
        pretty_assertions::assert_eq!(
            as_json.link_contexts.iter().map(ToString::to_string).collect::<Vec<_>>(),
            vec![NGSI_LD_CORE_CONTEXT_URL.to_string()]
        );
        let forwarded: Value = serde_json::from_slice(as_json.body.as_deref().expect("body")).expect("JSON body");
        let expanded = forwarded.as_array().and_then(|items| items.first()).expect("expanded document is an array");
        assert!(
            expanded.get("https://example.org/speed").is_some(),
            "the inline term is expanded to its IRI: {forwarded}"
        );
        assert!(expanded.get("@context").is_none());
    }

    #[tokio::test]
    async fn a_configured_jsonld_context_forces_application_json_and_compacts_against_it() {
        let core = NGSI_LD_CORE_CONTEXT_URL.to_string();
        let info_pairs = pairs(&[("jsonldContext", &core), ("contentType", "application/ld+json")]);
        let info = ContextSourceInfo::try_from(info_pairs.as_slice()).expect("valid info");
        // The inbound request names no context of its own, so expansion and
        // compaction both resolve offline against the embedded core context.
        let payload = build_forwarded_payload(&engine(), &ContextSpec::none(), Some(&body()), &info, None)
            .await
            .expect("payload builds");
        pretty_assertions::assert_eq!(payload.media_type, ResponseMediaType::Json);
        pretty_assertions::assert_eq!(payload.link_contexts.iter().map(ToString::to_string).collect::<Vec<_>>(), vec![core]);
        let forwarded: Value = serde_json::from_slice(payload.body.as_deref().expect("body")).expect("JSON body");
        assert!(forwarded.get("@context").is_none());
        assert!(forwarded.get("id").is_some(), "compacted against the core context: {forwarded}");
    }

    #[tokio::test]
    async fn a_body_less_request_carries_only_the_link_contexts() {
        let info_pairs = pairs(&[]);
        let info = ContextSourceInfo::try_from(info_pairs.as_slice()).expect("valid info");
        let payload = build_forwarded_payload(&engine(), &link_spec(&[USER_CONTEXT]), None, &info, None)
            .await
            .expect("payload builds");
        assert!(payload.body.is_none());
        pretty_assertions::assert_eq!(
            payload.link_contexts.iter().map(ToString::to_string).collect::<Vec<_>>(),
            vec![USER_CONTEXT.to_string()]
        );

        let none = build_forwarded_payload(&engine(), &ContextSpec::none(), None, &info, None)
            .await
            .expect("payload builds");
        assert_eq!(none.link_contexts, [] as [iri_rs::IriRef<String>; 0]);
    }

    #[tokio::test]
    async fn an_ngsild_conformance_applies_the_clause_4_3_6_8_fallbacks() {
        let info_pairs = pairs(&[("ngsildConformance", "1.0")]);
        let info = ContextSourceInfo::try_from(info_pairs.as_slice()).expect("valid info");
        let versioned = serde_json::to_vec(&json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "scope": "/Madrid",
            "speed": {"type": "Property", "value": 10, "observedAt": "2024-01-01T00:00:00Z"}
        }))
        .expect("serialises");
        let payload = build_forwarded_payload(&engine(), &link_spec(&[USER_CONTEXT]), Some(&versioned), &info, None)
            .await
            .expect("payload builds");
        let forwarded: Value = serde_json::from_slice(payload.body.as_deref().expect("body")).expect("JSON body");
        assert!(forwarded.get("scope").is_none(), "scope was introduced in 1.4: {forwarded}");
        assert!(forwarded["speed"].get("observedAt").is_none(), "observedAt was introduced in 1.3: {forwarded}");
    }
}
