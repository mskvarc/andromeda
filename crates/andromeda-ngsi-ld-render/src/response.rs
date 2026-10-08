//! The configuration every response is written under: the negotiated media
//! type, the `@context` in the one carrier the response uses, the version the
//! client asked for and the representation.
//!
//! A [`ResponseRenderer`] holds that configuration and nothing else. The body is
//! attached by the writer that knows its shape and is written straight from the
//! typed model into the response: [`ResponseRenderer::with_entities`] for
//! entities (see [`crate::entity_response`]), `with_body` and `with_collection`
//! for every other resource (see [`crate::body_response`]) and the temporal
//! writer for temporal evolutions (see [`crate::temporal::temporal_response`]).

use crate::{
    context::ResponseContext,
    entity_response::{EntityPayload, ReadyEntityResponse},
    geometry_selection::GeometrySelection,
    version::adaptation::VersionAdaptation,
};
use andromeda_http_headers::{language::LanguageFilter, media_type::ResponseMediaType};
use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;
use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
use iri_rs::IriBuf;

/// Configuration for writing a response, before its body.
///
/// Configured by whoever answered the request, from the negotiated media type,
/// the resolved `@context`, the client's `Prefer` tokens and -- for `GeoJSON` --
/// the attribute the geometry is taken from. It cannot be written on its own:
/// only a writer that takes a body produces something that carries `render_into`,
/// so a caller cannot forget the body -- the type system will not let it.
#[derive(Debug)]
pub struct ResponseRenderer<'a> {
    /// The negotiated media type, which decides the envelope.
    pub(crate) media_type: ResponseMediaType,
    /// The resolved `@context`, in the one carrier the response uses.
    pub(crate) context: ResponseContext,
    /// The `Prefer: ngsi-ld=` version to adapt the payload to, if any.
    pub(crate) requested_version: Option<SpecVersion>,
    /// The clause 4.5 representation the payload is written in.
    pub(crate) format: ResponseFormat,
    /// The clause 4.5.16.1 rule a `GeoJSON` geometry is selected by.
    pub(crate) geometry_selection: GeometrySelection<'a>,
}

/// What a written response says about itself beside its body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseHead {
    /// The target of the one context `Link` header the response carries, when
    /// its `@context` is advertised rather than embedded.
    pub context_link: Option<IriBuf>,
    /// The media type this response is written as.
    pub media_type: ResponseMediaType,
    /// Whether version adaptation was applied, which makes the status 203.
    pub version_adaptation: VersionAdaptation,
}

impl<'a> ResponseRenderer<'a> {
    /// Create a renderer for the given media type and resolved `@context`.
    #[must_use]
    pub const fn new(media_type: ResponseMediaType, context: ResponseContext) -> ResponseRenderer<'a> {
        ResponseRenderer {
            media_type,
            context,
            requested_version: None,
            format: ResponseFormat::Normalized,
            geometry_selection: GeometrySelection::new(None, &[]),
        }
    }

    /// Set the clause 4.5 representation the payload is written in.
    #[must_use]
    pub const fn with_format(mut self, format: ResponseFormat) -> Self {
        self.format = format;
        self
    }

    /// Set the `Prefer: ngsi-ld=` version the payload is adapted to, if any.
    ///
    /// A `GeoJSON` response keeps none: clauses 5.7.1.4 and 6.3.6 amend only
    /// `application/json` and `application/ld+json` payloads and echo
    /// `Preference-Applied` only for them, and the operation tables answer a
    /// `GeoJSON` `Feature` or `FeatureCollection` with 200 alone.
    #[must_use]
    pub const fn with_requested_version(mut self, requested_version: Option<SpecVersion>) -> Self {
        self.requested_version = match self.media_type {
            ResponseMediaType::Json | ResponseMediaType::JsonLd => requested_version,
            ResponseMediaType::GeoJson => None,
        };
        self
    }

    /// Set the clause 4.5.16.1 rule a `GeoJSON` geometry is selected by: the
    /// `geometryProperty` the request named and the `datasetId` values it
    /// narrowed the instances to.
    #[must_use]
    pub const fn with_geometry_selection(mut self, geometry_selection: GeometrySelection<'a>) -> Self {
        self.geometry_selection = geometry_selection;
        self
    }

    /// Attach the typed entities to write, in the representation set by
    /// [`ResponseRenderer::with_format`] and negotiated against
    /// `language_filter` (clause 4.15), producing a runnable
    /// [`ReadyEntityResponse`].
    ///
    /// The entities are written straight into the response body; see
    /// [`crate::entity_response`].
    #[must_use]
    pub const fn with_entities(self, payload: EntityPayload<'a>, language_filter: Option<&'a LanguageFilter>) -> ReadyEntityResponse<'a> {
        ReadyEntityResponse::new(self, payload, language_filter)
    }

    /// The version outcome of the requested `Prefer: ngsi-ld=` version, which
    /// makes the status 203 when the payload was written for an older one
    /// (clause 4.3.6.8).
    pub(crate) fn version_adaptation(&self) -> VersionAdaptation {
        self.requested_version.map_or(VersionAdaptation::None, VersionAdaptation::requested)
    }
}

#[cfg(test)]
mod tests {
    use crate::{context::ResponseContext, response::ResponseRenderer, version::adaptation::VersionAdaptation};
    use andromeda_http_headers::media_type::ResponseMediaType;
    use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;

    /// Clause 4.3.6.8: `Prefer: ngsi-ld=1.9` names the current version, so
    /// nothing is adapted and the response stays 200, not 203.
    #[test]
    fn a_conformant_target_version_yields_no_adaptation() {
        let renderer = ResponseRenderer::new(ResponseMediaType::Json, ResponseContext::Absent).with_requested_version(Some(SpecVersion::CONFORMANT));
        pretty_assertions::assert_eq!(renderer.version_adaptation(), VersionAdaptation::Unchanged);
    }

    #[test]
    fn an_older_target_version_is_reported_as_applied() {
        let renderer = ResponseRenderer::new(ResponseMediaType::Json, ResponseContext::Absent).with_requested_version(Some(SpecVersion::V1_4));
        pretty_assertions::assert_eq!(renderer.version_adaptation(), VersionAdaptation::Applied { version: SpecVersion::V1_4 });
    }

    #[test]
    fn a_geojson_response_ignores_an_older_target_version() {
        let renderer = ResponseRenderer::new(ResponseMediaType::GeoJson, ResponseContext::Absent).with_requested_version(Some(SpecVersion::V1_3));
        pretty_assertions::assert_eq!(renderer.version_adaptation(), VersionAdaptation::None);
    }

    #[test]
    fn no_requested_version_is_no_adaptation() {
        let renderer = ResponseRenderer::new(ResponseMediaType::Json, ResponseContext::Absent);
        pretty_assertions::assert_eq!(renderer.version_adaptation(), VersionAdaptation::None);
    }
}
