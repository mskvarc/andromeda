//! Starting a rendering from what a request handler holds.
//!
//! `andromeda-ngsi-ld-render` takes values: a media type, the `@context` in the
//! one carrier the response uses, and the `Prefer` tokens that change how a
//! body is written. A handler holds an axum extractor and the
//! [`PresentedContext`] its engine resolved from the request's `@context`
//! (clauses 5.2.3, 4.4 and 5.5.7), and this is the one place those are turned
//! into the other.
//!
//! Which carrier a response uses is the HTTP binding's decision (clause 6.3.6):
//! an `application/ld+json` body embeds its `@context`, an `application/json`
//! response advertises it in exactly one `Link` header, and a `GeoJSON` body
//! embeds it unless `Prefer: body=json` asks otherwise (clause 4.5.16). Making
//! that decision here, before rendering, is what lets a compound request
//! context be hosted only when a `Link` header has to name it (Annex C.8).

use andromeda_http_headers::{geo_json_body::GeoJsonBodyPreference, media_type::ResponseMediaType};
use andromeda_jsonld::{
    compound_context_host::{CompoundContextHost, ContextHostingError, WrapperUse},
    error::JsonLdError,
    presented_context::{ContextLinkTarget, PresentedContext},
};
use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;
use andromeda_ngsi_ld_render::{context::ResponseContext, response::ResponseRenderer};
use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
use iri_rs::IriBuf;

/// Configure a response renderer from a handler's media type, context,
/// preferences and the representation its payload is already rendered in.
///
/// Returns the renderer before its body; the caller attaches that with
/// `with_entities`, `with_body` or `with_collection`, which are the only routes
/// to a runnable response, so a handler cannot render a body without one.
///
/// `format` is a parameter rather than a `with_*` step because forgetting it is
/// not a missed optimization. Clause 4.3.6.8's fallback tables are applied to
/// the payload as it already stands, so a body rendered under clause 4.5.4 and
/// adapted under the normalized reading of clause 4.5.1 comes back corrupted;
/// requiring it here means a handler states which representation it rendered.
///
/// `host` is the tenant's hosting of `@context`s, asked only when the response
/// names its context in a `Link` header and the presented value is not one
/// string (Annex C.8); nothing is hosted for a response that embeds its
/// context. A caller renders a body with this; a response with no body, a 207
/// or an error names no context at all (clause 6.3.6) and never reaches it.
///
/// Use [`start_rendering_normalized`] for a resource whose representation the
/// client cannot select at all.
///
/// # Errors
///
/// Returns the [`ContextHostingError`] of a compound request context that
/// could not be hosted, which
/// [`crate::context_hosting_failure::classify_context_hosting_error`] classifies.
pub async fn start_rendering<'a>(
    media_type: ResponseMediaType,
    context: PresentedContext,
    host: &dyn CompoundContextHost,
    preferences: RenderingPreferences,
    format: ResponseFormat,
) -> Result<ResponseRenderer<'a>, ContextHostingError> {
    let carrier = ContextCarrier::of(media_type, preferences.geo_json_body);
    let context = response_context(context, host, carrier).await?;
    Ok(ResponseRenderer::new(media_type, context)
        .with_requested_version(preferences.requested_version)
        .with_format(format))
}

/// Configure a response renderer for a resource that has only the normalized
/// representation.
///
/// A Subscription (clauses 6.10 and 6.11), a Context Source Registration
/// (clauses 6.8 and 6.9), an `EntityMap` (clauses 6.32, 6.34 and 6.35), a
/// Snapshot (clauses 6.36 to 6.38), a batch operation result (clauses 6.14 to
/// 6.17 and 6.31) and the type and attribute information of clauses 6.25 to
/// 6.28 all answer in one shape: none of those resources takes a `format` or an
/// `options` parameter that would select another.
///
/// This exists so that a handler with nothing to decide states that, rather
/// than passing a representation it never chose. A temporal answer is written
/// by its own writer, which reads the clause 6.3.12 representation from the
/// payload, so it too starts here.
///
/// # Errors
///
/// As [`start_rendering`].
pub async fn start_rendering_normalized<'a>(
    media_type: ResponseMediaType,
    context: PresentedContext,
    host: &dyn CompoundContextHost,
    preferences: RenderingPreferences,
) -> Result<ResponseRenderer<'a>, ContextHostingError> {
    start_rendering(media_type, context, host, preferences, ResponseFormat::Normalized).await
}

/// Configure a response renderer for a resource that always answers under the
/// core context alone, in the normalized representation.
///
/// The core context is one URL, so its `Link` names it as is and nothing is
/// ever hosted.
///
/// # Errors
///
/// Returns [`JsonLdError::ContextText`] when the core context URL cannot be
/// written as a JSON string, which no URL fails to be.
pub fn start_rendering_core<'a>(media_type: ResponseMediaType, preferences: RenderingPreferences) -> Result<ResponseRenderer<'a>, JsonLdError> {
    let context = match ContextCarrier::of(media_type, preferences.geo_json_body) {
        ContextCarrier::Body => ResponseContext::Body(PresentedContext::core()?.into_literal()),
        ContextCarrier::Link => ResponseContext::Link(IriBuf::from(NGSI_LD_CORE_CONTEXT_URL)),
    };
    Ok(ResponseRenderer::new(media_type, context)
        .with_requested_version(preferences.requested_version)
        .with_format(ResponseFormat::Normalized))
}

/// Configure a response renderer for the body of a partial success.
///
/// Clause 6.3.6: "Operations that result in an error that return a payload or
/// that result in a partial success (207 Multi-Status) shall always respond
/// with MIME type "application/json", regardless of the Accept header. [...]
/// Only Fully Qualified Names shall be used in the payload body of error or
/// partial success responses, as there is no context present." The body is
/// therefore `application/json` and names no context, in the body or in a
/// `Link` header.
#[must_use]
pub const fn start_rendering_partial_success<'a>() -> ResponseRenderer<'a> {
    ResponseRenderer::new(ResponseMediaType::Json, ResponseContext::Absent)
}

/// Rendering-related values resolved from the request's typed `Prefer` header.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RenderingPreferences {
    /// Whether a `GeoJSON` response carries JSON-LD context in the body or a `Link` header.
    pub geo_json_body: GeoJsonBodyPreference,
    /// Requested earlier NGSI-LD representation version, when present.
    pub requested_version: Option<SpecVersion>,
}

/// Where a response carries its `@context` (clause 6.3.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContextCarrier {
    /// Embedded as the body's `@context` member.
    Body,
    /// Advertised by one `Link` header.
    Link,
}

impl ContextCarrier {
    /// The carrier clause 6.3.6 assigns a media type: `application/ld+json`
    /// embeds the context, `application/json` cannot and advertises it, and a
    /// `GeoJSON` envelope embeds it unless the client asked with
    /// `Prefer: body=json` for it not to (clause 4.5.16).
    const fn of(media_type: ResponseMediaType, geo_json_body: GeoJsonBodyPreference) -> ContextCarrier {
        match (media_type, geo_json_body) {
            (ResponseMediaType::JsonLd, GeoJsonBodyPreference::LdJson | GeoJsonBodyPreference::Json)
            | (ResponseMediaType::GeoJson, GeoJsonBodyPreference::LdJson) => ContextCarrier::Body,
            (ResponseMediaType::Json, GeoJsonBodyPreference::LdJson | GeoJsonBodyPreference::Json)
            | (ResponseMediaType::GeoJson, GeoJsonBodyPreference::Json) => ContextCarrier::Link,
        }
    }
}

/// The presented context in the one carrier the response uses.
///
/// A context is hosted only here, and only when the response names it in a
/// `Link` header and no single URL names it: a body embeds the value itself.
async fn response_context(context: PresentedContext, host: &dyn CompoundContextHost, carrier: ContextCarrier) -> Result<ResponseContext, ContextHostingError> {
    match carrier {
        ContextCarrier::Body => Ok(ResponseContext::Body(context.into_literal())),
        ContextCarrier::Link => match context.link_target() {
            ContextLinkTarget::Reference(target) => Ok(ResponseContext::Link(target)),
            ContextLinkTarget::Hosted(wrapped) => Ok(ResponseContext::Link(host.host(&wrapped, WrapperUse::Response).await?)),
        },
    }
}

#[cfg(test)]
mod tests {
    use crate::render::{ContextCarrier, RenderingPreferences, response_context, start_rendering_core, start_rendering_partial_success};
    use andromeda_http_headers::{geo_json_body::GeoJsonBodyPreference, media_type::ResponseMediaType};
    use andromeda_json_literal::JsonLiteral;
    use andromeda_jsonld::{
        compound_context_host::{CompoundContextHost, CompoundContextHostFuture, NoCompoundContextHost, WrapperUse},
        context_spec::ContextSpec,
        presented_context::{NoLoadedContexts, PresentedContext},
    };
    use andromeda_ngsi_ld_render::{
        context::ResponseContext,
        response::{ResponseHead, ResponseRenderer},
    };
    use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
    use iri_rs::IriBuf;
    use serde_json::{Value, json};
    use std::sync::Mutex;

    /// The URL [`RecordingHost`] answers for every hosted wrapper.
    const WRAPPER_URL: &str = "https://broker.example.com/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:JsonLdContext:wrapper";

    /// A host that records what it was asked to host and answers one fixed URL.
    #[derive(Debug, Default)]
    struct RecordingHost {
        hosted: Mutex<Vec<JsonLiteral>>,
    }

    impl RecordingHost {
        fn hosted(&self) -> Vec<JsonLiteral> {
            self.hosted.lock().expect("the recording lock").clone()
        }
    }

    impl CompoundContextHost for RecordingHost {
        fn host<'a>(&'a self, context: &'a JsonLiteral, _usage: WrapperUse) -> CompoundContextHostFuture<'a> {
            Box::pin(async move {
                self.hosted.lock().expect("the recording lock").push(context.clone());
                Ok(IriBuf::new(WRAPPER_URL.to_owned()).expect("a valid URL"))
            })
        }
    }

    fn iri(url: &str) -> IriBuf {
        IriBuf::new(url.to_owned()).expect("an absolute IRI")
    }

    async fn presented(spec: &ContextSpec) -> PresentedContext {
        PresentedContext::resolve(spec, &NoLoadedContexts).await.expect("nothing is loaded")
    }

    fn compound_spec() -> ContextSpec {
        ContextSpec::from_body(
            JsonLiteral::admit(&json!(["https://example.com/a.jsonld", "https://example.com/b.jsonld"]).to_string()).expect("an admissible literal"),
        )
    }

    /// Clause 6.3.6 and clause 4.5.16: which media type embeds the context and
    /// which advertises it.
    #[test]
    fn each_media_type_carries_its_context_where_clause_6_3_6_puts_it() {
        pretty_assertions::assert_eq!(
            ContextCarrier::of(ResponseMediaType::JsonLd, GeoJsonBodyPreference::LdJson),
            ContextCarrier::Body
        );
        pretty_assertions::assert_eq!(ContextCarrier::of(ResponseMediaType::Json, GeoJsonBodyPreference::LdJson), ContextCarrier::Link);
        pretty_assertions::assert_eq!(
            ContextCarrier::of(ResponseMediaType::GeoJson, GeoJsonBodyPreference::LdJson),
            ContextCarrier::Body
        );
        pretty_assertions::assert_eq!(
            ContextCarrier::of(ResponseMediaType::GeoJson, GeoJsonBodyPreference::Json),
            ContextCarrier::Link
        );
    }

    /// Clause 6.3.6: a presented value that is one string is linked as itself,
    /// and nothing is hosted.
    #[tokio::test]
    async fn a_link_naming_one_string_hosts_nothing() {
        let host = RecordingHost::default();

        let context = response_context(PresentedContext::core().expect("the core context"), &host, ContextCarrier::Link)
            .await
            .expect("a context");

        pretty_assertions::assert_eq!(context, ResponseContext::Link(iri(NGSI_LD_CORE_CONTEXT_URL.as_str())));
        pretty_assertions::assert_eq!(host.hosted(), Vec::<JsonLiteral>::new());
    }

    /// Annex C.8: a presented value of several contexts is linked through one
    /// hosted wrapper holding exactly that value.
    #[tokio::test]
    async fn a_compound_context_is_hosted_once_with_exactly_the_body_value() {
        let host = RecordingHost::default();
        let presented = presented(&compound_spec()).await;
        let body = presented.body().clone();

        let context = response_context(presented, &host, ContextCarrier::Link).await.expect("a context");

        pretty_assertions::assert_eq!(context, ResponseContext::Link(iri(WRAPPER_URL)));
        pretty_assertions::assert_eq!(host.hosted(), vec![body]);
        pretty_assertions::assert_eq!(
            host.hosted(),
            vec![
                JsonLiteral::admit(
                    &json!([
                        "https://example.com/a.jsonld",
                        "https://example.com/b.jsonld",
                        NGSI_LD_CORE_CONTEXT_URL.as_str()
                    ])
                    .to_string()
                )
                .expect("an admissible literal")
            ]
        );
    }

    /// An `application/ld+json` body embeds its context, so nothing is hosted
    /// whatever the context is.
    #[tokio::test]
    async fn a_body_carrier_never_hosts() {
        let host = RecordingHost::default();

        let context = response_context(presented(&compound_spec()).await, &host, ContextCarrier::Body)
            .await
            .expect("a context");

        pretty_assertions::assert_eq!(
            context,
            ResponseContext::Body(
                JsonLiteral::admit(
                    &json!([
                        "https://example.com/a.jsonld",
                        "https://example.com/b.jsonld",
                        NGSI_LD_CORE_CONTEXT_URL.as_str()
                    ])
                    .to_string()
                )
                .expect("an admissible literal")
            )
        );
        pretty_assertions::assert_eq!(host.hosted(), Vec::<JsonLiteral>::new());
    }

    #[tokio::test]
    async fn a_compound_context_the_host_cannot_store_is_a_hosting_failure() {
        let result = response_context(presented(&compound_spec()).await, &NoCompoundContextHost, ContextCarrier::Link).await;

        assert!(result.is_err(), "{result:?}");
    }

    /// Clause 6.3.6: a partial success names no context at all, so it never
    /// asks a host.
    /// The body and head a renderer writes for one resource.
    fn written(renderer: ResponseRenderer<'_>) -> (Value, ResponseHead) {
        let mut body = Vec::new();
        let head = renderer.with_body(&json!({ "id": "urn:x" })).render_into(&mut body).expect("a written body");
        (serde_json::from_slice(&body).expect("the body is JSON"), head)
    }

    #[test]
    fn a_partial_success_names_no_context() {
        let (body, head) = written(start_rendering_partial_success());

        pretty_assertions::assert_eq!(head.context_link, None);
        pretty_assertions::assert_eq!(head.media_type, ResponseMediaType::Json);
        pretty_assertions::assert_eq!(body.get("@context"), None);
    }

    #[test]
    fn a_core_resource_embeds_or_advertises_the_core_context_without_a_host() {
        let (embedded, embedded_head) = written(start_rendering_core(ResponseMediaType::JsonLd, RenderingPreferences::default()).expect("a renderer"));
        let (linked, linked_head) = written(start_rendering_core(ResponseMediaType::Json, RenderingPreferences::default()).expect("a renderer"));

        pretty_assertions::assert_eq!(embedded["@context"], json!(NGSI_LD_CORE_CONTEXT_URL.as_str()));
        pretty_assertions::assert_eq!(embedded_head.context_link, None);
        pretty_assertions::assert_eq!(linked_head.context_link, Some(iri(NGSI_LD_CORE_CONTEXT_URL.as_str())));
        assert!(linked.get("@context").is_none(), "{linked}");
    }
}
