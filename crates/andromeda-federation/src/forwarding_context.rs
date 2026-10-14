//! The one context a forwarded request is expressed against (clauses 4.3.6.6,
//! 6.3.5 and 6.3.19).
//!
//! A forwarded request names at most one context link: JSON-LD 1.1 section 6.1
//! admits one, and a source that follows the JSON-LD 1.1 API refuses a request
//! naming several ("multiple context link headers", section 9.4.1). Clause
//! 4.3.6.6 adds that a registration's `jsonldContext` is the context a
//! forwarded request is compacted against, "over both payload and query
//! parameters". So each forwarded request is written against exactly one
//! context, decided here once per registration, and every name and body it
//! carries is spelled in that context:
//!
//! - the registration's `jsonldContext`, when it names one;
//! - otherwise the inbound request's own context, when a single absolute URL
//!   names it (the inbound `Link`, or a body `@context` that is one string);
//! - otherwise the v1.9 core context: an inbound context of several entries,
//!   or one written inline, has no URL a `Link` header could name, so the
//!   forwarded request is written in the terms the core context gives it,
//!   which for a user term is its full IRI.
//!
//! A request forwarded as `application/ld+json` carries its context inline
//! instead (clause 6.3.5), which is the inbound context as the client wrote it,
//! and no context link. A relative reference in that context names a document
//! relative to this broker's request, which a source would resolve against its
//! own URL instead, and the forwarding layer holds no base to resolve it
//! against; such a context is therefore never embedded, and the request is
//! written inline against the v1.9 core context, which spells every user term
//! as its full IRI. No wrapper is ever hosted for federation: a source answers
//! to this broker, which reads the answer back itself.

use crate::{error::FederationError, federation_invariant::FederationInvariant};
use andromeda_http_headers::media_type::ResponseMediaType;
use andromeda_json_literal::{
    JsonLiteral,
    piece::{JsonKind, JsonPiece},
};
use andromeda_jsonld::{batch_contexts::BatchContext, context_spec::ContextSpec, error::JsonLdError, request_context::RequestContext};
use andromeda_ngsi_ld_model::context_source_info::ContextSourceInfo;
use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
use iri_rs::{Iri, IriBuf};
use mediatype::{MediaType, MediaTypeBuf};

/// The JSON-LD 1.1 keyword a context definition imports another context with
/// (JSON-LD 1.1 section 4.1.10).
const IMPORT_KEYWORD: &str = "@import";

/// Whether the forwarded request carries a body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BodyPresence {
    /// A body-less request (a GET, a DELETE): its context can only travel in a
    /// `Link` header (clause 6.3.5).
    Absent,
    /// A request with a body, whose media type decides how its context travels.
    Present,
}

/// Whether the names a forwarded request carries differ from the inbound
/// request's spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Rewording {
    /// The forwarding context is the inbound context: every name travels as the
    /// client wrote it, and a source's answer is already in the client's terms.
    AsInbound,
    /// The forwarding context is another one: every name is expanded against
    /// the inbound context and compacted against the forwarding one, and a
    /// source's answer is read back the opposite way.
    Reword,
}

/// The single context one forwarded request is expressed against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ForwardingContext {
    /// The registration's `jsonldContext` (clause 4.3.6.6).
    Configured(IriBuf),
    /// The inbound request's own context, one absolute URL.
    InboundLink(IriBuf),
    /// The inbound request named no context: the default context, which no
    /// `Link` header has to name (clause 6.3.5).
    InboundDefault,
    /// The v1.9 core context, standing in for an inbound context no single
    /// `Link` header can name.
    CoreForCompound,
    /// The inbound context carried inline in an `application/ld+json` body
    /// (clause 6.3.5); every reference it holds is absolute.
    InlineInbound(JsonLiteral),
    /// The contexts of a batch whose entities name different ones, each
    /// carried inline on its own entity of an `application/ld+json` array
    /// (clause 6.3.5, and JSON-LD applying a top-level node's `@context` to that
    /// node alone); every reference they hold is absolute.
    InlinePerItem,
    /// The v1.9 core context carried inline in an `application/ld+json` body,
    /// standing in for an inbound context that holds a relative reference no
    /// source could resolve the way this broker does.
    InlineCore,
}

impl ForwardingContext {
    /// The context `Link` the forwarded request carries, if any.
    pub(crate) fn link(&self) -> Option<IriBuf> {
        match self {
            ForwardingContext::Configured(url) | ForwardingContext::InboundLink(url) => Some(url.clone()), // clone: the payload owns its Link IRI
            ForwardingContext::CoreForCompound => Some(IriBuf::from(NGSI_LD_CORE_CONTEXT_URL)),
            ForwardingContext::InboundDefault | ForwardingContext::InlineInbound(_) | ForwardingContext::InlinePerItem | ForwardingContext::InlineCore => None,
        }
    }

    /// Whether names are reworded between the inbound and the forwarding
    /// context.
    pub(crate) const fn rewording(&self) -> Rewording {
        match self {
            ForwardingContext::Configured(_) | ForwardingContext::CoreForCompound | ForwardingContext::InlineCore => Rewording::Reword,
            ForwardingContext::InboundLink(_) | ForwardingContext::InboundDefault | ForwardingContext::InlineInbound(_) | ForwardingContext::InlinePerItem => {
                Rewording::AsInbound
            }
        }
    }

    /// The context the forwarded request's names are spelled in.
    pub(crate) fn naming(&self, inbound: &ContextSpec) -> ContextSpec {
        match self {
            ForwardingContext::Configured(url) => ContextSpec::from_link(url.clone()), // clone: the spec owns its Link IRI
            ForwardingContext::CoreForCompound | ForwardingContext::InlineCore => ContextSpec::none(),
            ForwardingContext::InboundLink(_) | ForwardingContext::InboundDefault | ForwardingContext::InlineInbound(_) | ForwardingContext::InlinePerItem => {
                inbound.clone() // clone: the names are spelled in the inbound context itself
            }
        }
    }

    /// The `@context` an `application/ld+json` body carries inline for an entity
    /// submitted under `item`, if this context travels inline (clause 6.3.5).
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::JsonLd`] when a context URL cannot be written
    /// as a JSON string, which no IRI fails to be.
    pub(crate) fn inline_for(&self, item: &ContextSpec) -> Result<Option<JsonLiteral>, FederationError> {
        match self {
            ForwardingContext::InlineInbound(context) => Ok(Some(context.clone())), // clone: every forwarded entity owns its inline context
            ForwardingContext::InlinePerItem => inline_context(item).map(Some),
            ForwardingContext::InlineCore => written_reference(NGSI_LD_CORE_CONTEXT_URL.as_str()).map(Some),
            ForwardingContext::Configured(_) | ForwardingContext::InboundLink(_) | ForwardingContext::InboundDefault | ForwardingContext::CoreForCompound => {
                Ok(None)
            }
        }
    }
}

/// How one forwarded request is sent: its media type and the context it is
/// expressed against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ForwardingDecision {
    /// The media type of the forwarded request (clause 6.3.19).
    pub(crate) media_type: ResponseMediaType,
    /// The context the request is expressed against.
    pub(crate) context: ForwardingContext,
}

/// Decide how a request forwarded to a registration carrying `info` is sent,
/// for an inbound request under `inbound`.
///
/// # Errors
///
/// Returns [`FederationInvariant::RegistrationContentType`] when the
/// registration's admitted `contentType` is not a forwardable media type, and
/// [`FederationError::JsonLd`] when the inbound context cannot be written
/// inline.
pub(crate) fn decide(inbound: &ContextSpec, info: &ContextSourceInfo<'_>, presence: BodyPresence) -> Result<ForwardingDecision, FederationError> {
    let configured = info.jsonld_context.clone(); // clone: the decision owns the IRI the registration keeps
    // Clause 6.3.19: a configured `jsonldContext` forces `application/json`,
    // the one form that names a context in a `Link` header; otherwise
    // `contentType` decides and the default is `application/json`.
    let media_type = match (&configured, &info.content_type) {
        (Some(_), Some(_) | None) | (None, None) => ResponseMediaType::Json,
        (None, Some(content_type)) => forwardable_media_type(content_type)?,
    };
    let context = match (configured, media_type, presence) {
        (Some(url), _, _) => ForwardingContext::Configured(url),
        (None, ResponseMediaType::JsonLd, BodyPresence::Present) => {
            let inline = inline_context(inbound)?;
            if holds_only_absolute_references(inline.piece()) {
                ForwardingContext::InlineInbound(inline)
            } else {
                ForwardingContext::InlineCore
            }
        }
        (None, ResponseMediaType::JsonLd | ResponseMediaType::Json | ResponseMediaType::GeoJson, BodyPresence::Absent | BodyPresence::Present) => {
            inbound_link_context(inbound)
        }
    };
    Ok(ForwardingDecision { media_type, context })
}

/// The media type a registration's admitted `contentType` forwards a request
/// as.
///
/// # Errors
///
/// Returns [`FederationInvariant::RegistrationContentType`] for a media type
/// clause 4.3.6.6 would not have admitted.
fn forwardable_media_type(content_type: &MediaTypeBuf) -> Result<ResponseMediaType, FederationError> {
    let parsed: MediaType<'_> = content_type.into();
    ResponseMediaType::from_media_type(&parsed).ok_or_else(|| {
        FederationInvariant::RegistrationContentType {
            media_type: content_type.clone(), // clone: the error owns the media type it names
        }
        .into()
    })
}

/// Whether every context reference in an inline `@context` -- the strings of
/// the value or its array, and the `@import` of every definition -- is an
/// absolute IRI, so a source reads each exactly as this broker does.
fn holds_only_absolute_references(context: JsonPiece<'_>) -> bool {
    match context.kind() {
        JsonKind::String => context.as_str().is_some_and(|reference| Iri::parse(reference.as_ref()).is_ok()),
        JsonKind::Array => context.elements().unwrap_or_default().into_iter().all(holds_only_absolute_references),
        JsonKind::Object => match context.member(IMPORT_KEYWORD) {
            Some(imported) if imported.kind() == JsonKind::String => imported.as_str().is_some_and(|imported| Iri::parse(imported.as_ref()).is_ok()),
            Some(_) | None => true,
        },
        JsonKind::Null | JsonKind::Boolean | JsonKind::Number => true,
    }
}

/// Decide how a batch array whose entities were submitted under `contexts` is
/// forwarded to a registration carrying `info`.
///
/// Entities naming one context are forwarded exactly as a single request under
/// it. Entities naming different ones have no single context a `Link` header
/// could name: under `application/json` they are written against the core
/// context, each spelled from its own context; under `application/ld+json`
/// each carries its own inline.
///
/// # Errors
///
/// As [`decide`].
pub(crate) fn decide_for_items<'c>(
    contexts: impl IntoIterator<Item = &'c BatchContext>,
    info: &ContextSourceInfo<'_>,
) -> Result<ForwardingDecision, FederationError> {
    // The batch parser gave every distinct context its identity once, so the
    // distinct ones are told apart by that identity and never compared again.
    let mut distinct: Vec<&BatchContext> = Vec::new();
    for context in contexts {
        if distinct.iter().all(|seen| seen.id() != context.id()) {
            distinct.push(context);
        }
    }
    match distinct.as_slice() {
        [] => decide(&ContextSpec::none(), info, BodyPresence::Present),
        [only] => decide(only.spec(), info, BodyPresence::Present),
        [_, _, ..] => decide_mixed(&distinct, info),
    }
}

/// The decision for a batch whose entities name the different `contexts`.
fn decide_mixed(contexts: &[&BatchContext], info: &ContextSourceInfo<'_>) -> Result<ForwardingDecision, FederationError> {
    let decision = decide(&ContextSpec::none(), info, BodyPresence::Present)?;
    let context = match decision.context {
        configured @ ForwardingContext::Configured(_) => configured,
        ForwardingContext::InlineInbound(_) | ForwardingContext::InlinePerItem | ForwardingContext::InlineCore => {
            let mut inline = contexts.iter().map(|context| inline_context(context.spec()));
            if inline.try_fold(true, |all, context| {
                context.map(|context| all && holds_only_absolute_references(context.piece()))
            })? {
                ForwardingContext::InlinePerItem
            } else {
                ForwardingContext::InlineCore
            }
        }
        ForwardingContext::InboundLink(_) | ForwardingContext::InboundDefault | ForwardingContext::CoreForCompound => ForwardingContext::CoreForCompound,
    };
    Ok(ForwardingDecision {
        media_type: decision.media_type,
        context,
    })
}

/// The forwarding context the inbound request's own context gives a request
/// whose context travels in a `Link` header.
fn inbound_link_context(inbound: &ContextSpec) -> ForwardingContext {
    match inbound.request_context() {
        RequestContext::Default => ForwardingContext::InboundDefault,
        RequestContext::Link(url) => ForwardingContext::InboundLink(url.clone()), // clone: the forwarding context owns its IRI
        RequestContext::Body(context) => match single_url(context.piece()) {
            Some(url) => ForwardingContext::InboundLink(url),
            None => ForwardingContext::CoreForCompound,
        },
    }
}

/// The one absolute URL a body `@context` names, when it names exactly one:
/// a string, or an array holding one string.
fn single_url(context: JsonPiece<'_>) -> Option<IriBuf> {
    let reference = match context.kind() {
        JsonKind::String => context,
        JsonKind::Array => match context.elements()?.as_slice() {
            [only] if only.kind() == JsonKind::String => *only,
            [] | [_] | [_, _, ..] => return None,
        },
        JsonKind::Null | JsonKind::Boolean | JsonKind::Number | JsonKind::Object => return None,
    };
    IriBuf::new(reference.as_str()?.into_owned()).ok()
}

/// The inbound context written inline, as the client wrote it; the core
/// context when the client named none, so the forwarded body names one.
///
/// # Errors
///
/// Returns [`FederationError::JsonLd`] when a context URL cannot be written as
/// a JSON string, which no IRI fails to be.
pub(crate) fn inline_context(inbound: &ContextSpec) -> Result<JsonLiteral, FederationError> {
    match inbound.request_context() {
        RequestContext::Default => written_reference(NGSI_LD_CORE_CONTEXT_URL.as_str()),
        RequestContext::Link(url) => written_reference(url.as_str()),
        RequestContext::Body(context) => Ok(context.clone()), // clone: the forwarded body owns its inline context
    }
}

/// A context reference written as the JSON string a `@context` member holds.
fn written_reference(url: &str) -> Result<JsonLiteral, FederationError> {
    JsonLiteral::string(url).map_err(|error| FederationError::from(JsonLdError::ContextText(error)))
}

#[cfg(test)]
mod tests {
    use crate::{
        error::FederationError,
        federation_invariant::FederationInvariant,
        forwarding_context::{BodyPresence, ForwardingContext, Rewording, decide, forwardable_media_type},
    };
    use andromeda_http_headers::media_type::ResponseMediaType;
    use andromeda_json_literal::JsonLiteral;
    use andromeda_jsonld::context_spec::ContextSpec;
    use andromeda_ngsi_ld_model::{context_source_info::ContextSourceInfo, key_value_pair::KeyValuePair};
    use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
    use iri_rs::IriBuf;
    use mediatype::MediaTypeBuf;
    use serde_json::json;

    const USER: &str = "https://example.org/user.jsonld";
    const OTHER: &str = "https://example.org/other.jsonld";
    const CONFIGURED: &str = "https://example.org/configured.jsonld";

    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value.to_owned()).expect("a valid IRI")
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

    fn decided(inbound: &ContextSpec, entries: &[(&str, &str)], presence: BodyPresence) -> (ResponseMediaType, ForwardingContext) {
        let raw = pairs(entries);
        let info = ContextSourceInfo::try_from(raw.as_slice()).expect("valid info");
        let decision = decide(inbound, &info, presence).expect("decides");
        (decision.media_type, decision.context)
    }

    /// Clause 4.3.6.6: a registration's `jsonldContext` wins over whatever the
    /// inbound request named, and forces `application/json` (clause 6.3.19).
    #[test]
    fn a_configured_context_is_the_forwarding_context_whatever_the_inbound_one() {
        for inbound in [
            ContextSpec::none(),
            ContextSpec::from_link(iri(USER)),
            ContextSpec::from_body(JsonLiteral::admit(&json!([USER, OTHER]).to_string()).expect("an admissible literal")),
        ] {
            let (media, context) = decided(
                &inbound,
                &[("jsonldContext", CONFIGURED), ("contentType", "application/ld+json")],
                BodyPresence::Present,
            );
            pretty_assertions::assert_eq!(media, ResponseMediaType::Json);
            pretty_assertions::assert_eq!(context, ForwardingContext::Configured(iri(CONFIGURED)));
            pretty_assertions::assert_eq!(context.link(), Some(iri(CONFIGURED)));
            pretty_assertions::assert_eq!(context.rewording(), Rewording::Reword);
        }
    }

    /// A single inbound URL, from a `Link` header or a body string, is the
    /// forwarding context and needs no rewording.
    #[test]
    fn a_single_inbound_url_is_forwarded_as_its_own_link() {
        for inbound in [
            ContextSpec::from_link(iri(USER)),
            ContextSpec::from_body(JsonLiteral::admit(&json!(USER).to_string()).expect("an admissible literal")),
            ContextSpec::from_body(JsonLiteral::admit(&json!([USER]).to_string()).expect("an admissible literal")),
        ] {
            let (media, context) = decided(&inbound, &[], BodyPresence::Present);
            pretty_assertions::assert_eq!(media, ResponseMediaType::Json);
            pretty_assertions::assert_eq!(context, ForwardingContext::InboundLink(iri(USER)));
            pretty_assertions::assert_eq!(context.link(), Some(iri(USER)));
            pretty_assertions::assert_eq!(context.rewording(), Rewording::AsInbound);
        }
    }

    /// Several inbound URLs, or an inline definition, have no URL a single
    /// `Link` header names, so the request is written against the core context.
    #[test]
    fn a_compound_inbound_context_is_forwarded_against_the_core_context() {
        for inbound in [
            ContextSpec::from_body(JsonLiteral::admit(&json!([USER, OTHER]).to_string()).expect("an admissible literal")),
            ContextSpec::from_body(JsonLiteral::admit(&json!({"speed": "https://example.org/speed"}).to_string()).expect("an admissible literal")),
            ContextSpec::from_body(JsonLiteral::admit(&json!([USER, {"speed": "https://example.org/speed"}]).to_string()).expect("an admissible literal")),
        ] {
            let (_, context) = decided(&inbound, &[], BodyPresence::Absent);
            pretty_assertions::assert_eq!(context, ForwardingContext::CoreForCompound);
            pretty_assertions::assert_eq!(context.link(), Some(IriBuf::from(NGSI_LD_CORE_CONTEXT_URL)));
            pretty_assertions::assert_eq!(context.rewording(), Rewording::Reword);
            pretty_assertions::assert_eq!(context.naming(&inbound), ContextSpec::none());
        }
    }

    /// An inbound request naming no context forwards none either (clause
    /// 6.3.5: its absence is the default context).
    #[test]
    fn a_default_inbound_context_is_forwarded_without_a_link() {
        let (_, context) = decided(&ContextSpec::none(), &[], BodyPresence::Present);
        pretty_assertions::assert_eq!(context, ForwardingContext::InboundDefault);
        assert!(context.link().is_none());
        pretty_assertions::assert_eq!(context.rewording(), Rewording::AsInbound);
    }

    /// Clause 6.3.5: an `application/ld+json` body carries its context inline,
    /// as the client wrote it; a body-less request still names its context in
    /// a `Link` header.
    #[test]
    fn an_ld_json_body_carries_the_inbound_context_inline() {
        let inbound = ContextSpec::from_body(JsonLiteral::admit(&json!([USER, OTHER]).to_string()).expect("an admissible literal"));
        let ld = [("contentType", "application/ld+json")];
        let (media, with_body) = decided(&inbound, &ld, BodyPresence::Present);
        pretty_assertions::assert_eq!(media, ResponseMediaType::JsonLd);
        pretty_assertions::assert_eq!(
            with_body,
            ForwardingContext::InlineInbound(JsonLiteral::admit(&json!([USER, OTHER]).to_string()).expect("an admissible literal"))
        );
        assert!(with_body.link().is_none());
        pretty_assertions::assert_eq!(with_body.rewording(), Rewording::AsInbound);

        let (_, body_less) = decided(&inbound, &ld, BodyPresence::Absent);
        pretty_assertions::assert_eq!(body_less, ForwardingContext::CoreForCompound);

        let (_, default_inline) = decided(&ContextSpec::none(), &ld, BodyPresence::Present);
        pretty_assertions::assert_eq!(
            default_inline,
            ForwardingContext::InlineInbound(JsonLiteral::admit(&json!(NGSI_LD_CORE_CONTEXT_URL.to_string()).to_string()).expect("an admissible literal"))
        );
    }

    /// Clause 4.3.6.6 admits only `application/json` and `application/ld+json`
    /// as a `contentType`, so any other media type reaching the decision is a
    /// broken invariant, never a silent fallback to `application/json`.
    #[test]
    fn a_content_type_no_forwarded_request_can_carry_is_a_broken_invariant() {
        pretty_assertions::assert_eq!(
            forwardable_media_type(&MediaTypeBuf::from_string("application/ld+json".to_owned()).expect("a media type")).ok(),
            Some(ResponseMediaType::JsonLd)
        );
        let refused = forwardable_media_type(&MediaTypeBuf::from_string("text/plain".to_owned()).expect("a media type"));
        assert!(
            matches!(refused, Err(FederationError::Internal(ref invariant)) if matches!(**invariant, FederationInvariant::RegistrationContentType { .. })),
            "{refused:?}"
        );
    }
}
