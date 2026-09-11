//! Starting a rendering from what a request handler holds.
//!
//! `andromeda-ngsi-ld-render` takes values: a media type, a resolved
//! `@context`, and the `Prefer` tokens that change how a payload is written.
//! A handler holds an axum extractor and a [`ContextSpec`], and this is the one
//! place those are turned into the other. Nothing else in the tree resolves a
//! `ContextSpec` for presentation, so the body value and the `Link` URLs cannot
//! come from two different readings of it.

use andromeda_http_headers::{geo_json_body::GeoJsonBodyPreference, media_type::ResponseMediaType};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{spec_version::SpecVersion, temporal_format::TemporalFormat};
use andromeda_ngsi_ld_render::{context::ResponseContext, response::ResponseRenderer, version::adapted_payload::AdaptedPayload};
use andromeda_ngsi_ld_representation::response_format::ResponseFormat;

/// Configure a response renderer from a handler's media type, context,
/// preferences and the representation its payload is already rendered in.
///
/// Returns the renderer before its payload; the caller attaches that with
/// [`ResponseRenderer::with_payload`], which is the only route to a runnable
/// response, so a handler cannot render a body without one.
///
/// `format` is a parameter rather than a `with_*` step because forgetting it is
/// not a missed optimization. Clause 4.3.6.8's fallback tables are applied to
/// the payload as it already stands, so a body rendered under clause 4.5.4 and
/// adapted under the normalized reading of clause 4.5.1 comes back corrupted;
/// requiring it here means a handler states which representation it rendered.
///
/// Use [`start_rendering_normalized`] for a resource whose representation the
/// client cannot select at all.
#[must_use]
pub fn start_rendering<'a>(
    media_type: ResponseMediaType,
    context: &ContextSpec,
    preferences: RenderingPreferences,
    format: ResponseFormat,
) -> ResponseRenderer<'a> {
    ResponseRenderer::new(media_type, response_context(context))
        .with_geo_json_body(preferences.geo_json_body)
        .with_requested_version(preferences.requested_version)
        .with_format(format)
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
/// than passing a representation it never chose. A temporal answer belongs to
/// [`start_rendering_temporal`] instead: it is reified the same way, but two of
/// the clause 4.3.6.8 fallbacks read differently on it.
#[must_use]
pub fn start_rendering_normalized<'a>(media_type: ResponseMediaType, context: &ContextSpec, preferences: RenderingPreferences) -> ResponseRenderer<'a> {
    start_rendering(media_type, context, preferences, ResponseFormat::Normalized)
}

/// Configure a response renderer for the Temporal Evolution of an Entity
/// (clause 4.5.6).
///
/// The temporal representation is reified in the same way an
/// [`ResponseFormat::Normalized`] Entity is -- its own `temporalValues` and
/// `aggregatedValues` renderings change what an Attribute instance holds and
/// not whether the Attribute is an object naming its `type` -- so it is read
/// under the normalized representation, and the client cannot select another.
///
/// What it does not share with an Entity is how clause 4.3.6.8 reads an
/// Attribute's array and its `observedAt`, which is why a temporal answer says
/// so here rather than reaching for [`start_rendering_normalized`]. `format` is
/// the clause 6.3.12 representation the answer was rendered in, because the two
/// simplified ones carry the sequence in time inside a member of the attribute
/// rather than as the attribute's own array. See [`AdaptedPayload`].
#[must_use]
pub fn start_rendering_temporal<'a>(
    media_type: ResponseMediaType,
    context: &ContextSpec,
    preferences: RenderingPreferences,
    format: TemporalFormat,
) -> ResponseRenderer<'a> {
    start_rendering_normalized(media_type, context, preferences).with_adapted_payload(AdaptedPayload::of_temporal(format))
}

/// Rendering-related values resolved from the request's typed `Prefer` header.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RenderingPreferences {
    /// Whether a `GeoJSON` response carries JSON-LD context in the body or a `Link` header.
    pub geo_json_body: GeoJsonBodyPreference,
    /// Requested earlier NGSI-LD representation version, when present.
    pub requested_version: Option<SpecVersion>,
}

/// Resolve a context spec into the two spellings a response needs.
///
/// Both come from a single [`ContextSpec::for_presentation`] reading, so a
/// response cannot advertise one set of contexts in its `Link` header and embed
/// a different set in its body.
#[must_use]
pub fn response_context(context: &ContextSpec) -> ResponseContext {
    let presentation = context.for_presentation();
    ResponseContext::new(presentation.to_context_value(), presentation.to_link_headers())
}

#[cfg(test)]
mod tests {
    use crate::render::response_context;
    use andromeda_jsonld::context_spec::ContextSpec;
    use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
    use iri_rs::IriRefBuf;

    /// A context URL as the [`IriRefBuf`] a `Link` header carries.
    fn iri_ref(url: &str) -> IriRefBuf {
        IriRefBuf::new(url.to_string()).expect("a valid IRI reference")
    }

    #[test]
    fn the_body_value_and_the_link_urls_come_from_one_presentation_reading() {
        // Two `for_presentation()` calls used to resolve this, one in the
        // renderer and one in the header builder. They agree here by
        // construction rather than by both being written the same way.
        let spec = ContextSpec::from_links(vec![iri_ref("https://example.com/context.jsonld")]);
        let presentation = spec.for_presentation();
        let mut resolved = response_context(&spec);

        pretty_assertions::assert_eq!(resolved.take_body_value(), presentation.to_context_value());
        pretty_assertions::assert_eq!(resolved.link_urls(), presentation.to_link_headers().as_slice());
    }

    #[test]
    fn a_spec_with_no_explicit_context_still_advertises_the_core_context() {
        let resolved = response_context(&ContextSpec::none());
        assert!(
            resolved.link_urls().iter().any(|url| NGSI_LD_CORE_CONTEXT_URL == url.as_str()),
            "the core context is missing from {:?}",
            resolved.link_urls()
        );
    }
}
