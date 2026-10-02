//! The response pipeline: everything that happens to a payload after the
//! broker has decided what to answer and before it is written to the wire.
//!
//! Four transformations, in this order, none of them optional in the sense
//! that a caller could reorder them:
//!
//! 1. percent-decoding of the URIs a Relationship points at,
//! 2. version adaptation for a client that asked for an older spec version
//!    (`Prefer: ngsi-ld=<version>`, clause 4.3.6.8),
//! 3. `@context` injection for a body that carries it (clause 6.3.6), and
//! 4. the `GeoJSON` Feature or `FeatureCollection` envelope (clause 4.5.16).
//!
//! Which of them run is a function of the media type and the payload shape,
//! and of nothing else — which is why there is one pipeline here rather than
//! one per shape. The differences are declared as [`ResponsePayload`]'s three
//! variants and read once.

use crate::{
    context::ResponseContext,
    entity_response::{EntityPayload, ReadyEntityResponse},
    error::RenderError,
    geojson::{GeoJsonRenderer, GeoJsonShape},
    geometry_selection::GeometrySelection,
    uri_decoding::decode_percent_encoded_uris,
    version::{adaptation::VersionAdaptation, adapted_payload::AdaptedPayload, renderer::VersionRenderer},
};
use andromeda_http_headers::{language::LanguageFilter, media_type::ResponseMediaType};
use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;
use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
use iri_rs::IriBuf;
use serde_json::Value;
use std::sync::Arc;

/// Payload fed to [`ResponseRenderer`]: the three NGSI-LD response shapes.
///
/// The three variants are *not* interchangeable: batch results skip
/// percent-encoded URI decoding and never participate in `GeoJSON` wrapping,
/// while collections unwrap into JSON arrays with per-item `@context`
/// injection in `ld+json`.
#[derive(Debug)]
pub enum ResponsePayload {
    /// Single entity/object (e.g. GET `/entities/{id}`, GET `/types`).
    Single(Value),
    /// Array of entities (e.g. GET `/entities`, GET `/subscriptions`).
    Collection(Vec<Value>),
    /// Batch operation result (BatchOperationResult-shaped object).
    Batch(Value),
}

impl ResponsePayload {
    /// Whether this shape carries Relationship objects worth decoding.
    ///
    /// A batch result names entity identifiers in `success` and `errors`
    /// arrays; it holds no attributes, so there is nothing to walk.
    const fn carries_relationships(&self) -> bool {
        matches!(self, ResponsePayload::Single(_) | ResponsePayload::Collection(_))
    }

    /// The `GeoJSON` envelope this shape takes, if it takes one.
    ///
    /// Clause 4.5.16 wraps entities. A batch result is not an entity and is
    /// answered as `application/json` whatever the request asked for, which is
    /// what `None` says here.
    const fn geo_json_shape(&self) -> Option<GeoJsonShape> {
        match self {
            ResponsePayload::Single(_) => Some(GeoJsonShape::Feature),
            ResponsePayload::Collection(_) => Some(GeoJsonShape::FeatureCollection),
            ResponsePayload::Batch(_) => None,
        }
    }

    /// The payload as one JSON value.
    fn into_value(self) -> Value {
        match self {
            ResponsePayload::Single(value) | ResponsePayload::Batch(value) => value,
            ResponsePayload::Collection(values) => Value::Array(values),
        }
    }
}

/// Configuration for the response rendering pipeline, before the payload.
///
/// Configured by whoever answered the request, from the negotiated media type,
/// the resolved `@context`, the client's `Prefer` tokens and — for `GeoJSON` —
/// the attribute the geometry is taken from. It cannot be rendered on its own:
/// only [`ResponseRenderer::with_payload`] produces a [`ReadyResponse`], and
/// only that carries [`ReadyResponse::render`]. A caller therefore cannot forget
/// the payload — the type system will not let it — which is why the payload is
/// not an optional field defaulted to a JSON null.
#[derive(Debug)]
pub struct ResponseRenderer<'a> {
    /// The negotiated media type, which decides the envelope.
    pub(crate) media_type: ResponseMediaType,
    /// The resolved `@context`, in the one carrier the response uses.
    pub(crate) context: ResponseContext,
    /// The `Prefer: ngsi-ld=` version to adapt the payload to, if any.
    pub(crate) requested_version: Option<SpecVersion>,
    /// The clause 4.5 representation the payload is rendered in.
    pub(crate) format: ResponseFormat,
    /// Which NGSI-LD Element the payload is, which version adaptation reads.
    pub(crate) adapted_payload: AdaptedPayload,
    /// The clause 4.5.16.1 rule a `GeoJSON` geometry is selected by.
    pub(crate) geometry_selection: GeometrySelection<'a>,
}

/// A fully-configured renderer with its payload attached, ready to run.
///
/// The only value that carries [`ReadyResponse::render`]; it exists solely
/// because a [`ResponseRenderer`] gained its payload, so a response body can
/// never be rendered without one.
#[derive(Debug)]
pub struct ReadyResponse<'a> {
    /// The configuration the payload was attached to.
    pub(crate) renderer: ResponseRenderer<'a>,
    /// What is being answered.
    pub(crate) payload: ResponsePayload,
    /// Whether the payload's Relationship targets are still to be decoded.
    pub(crate) targets: TargetDecoding,
}

/// Whether a payload's Relationship targets have been percent-decoded yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TargetDecoding {
    /// The pipeline decodes them, as it does every document a caller hands it.
    Pending,
    /// The typed walk that wrote the payload decoded them as it wrote them.
    Applied,
}

/// Output of the rendering pipeline, ready for the response type to serialize.
#[derive(Debug)]
pub struct RenderedOutput {
    /// The fully-rendered JSON value, with `@context` injected if applicable.
    pub value: Value,
    /// How the response carrying it is written.
    pub head: ResponseHead,
}

/// What a rendered response says about itself beside its body.
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
            adapted_payload: AdaptedPayload::Entity,
            geometry_selection: GeometrySelection::new(None, &[]),
        }
    }

    /// Set the clause 4.5 representation the payload was already rendered in.
    ///
    /// Only version adaptation reads it, and only because clause 4.3.6.8's
    /// fallback tables are applied to whatever the payload already looks like:
    /// clause 4.5.1 reifies an attribute, clause 4.5.2.3 leaves its family to be
    /// inferred, and clause 4.5.4 reduces it to a bare value or one of three
    /// one-member wrappers. A payload rendered under clause 4.5.4 and adapted
    /// under the normalized reading comes back with a `{"value": ...}` object
    /// that clause defines for nothing, so this is not an optimization: the
    /// representation the payload is in has to be stated.
    #[must_use]
    pub const fn with_format(mut self, format: ResponseFormat) -> Self {
        self.format = format;
        self
    }

    /// Set which NGSI-LD Element the payload is.
    ///
    /// Only version adaptation reads it, and only for the two rows of the
    /// clause 4.3.6.8 tables that read differently once an attribute's array is
    /// the clause 4.5.7 sequence of instances in time. Every resource whose
    /// payload is an Entity leaves it at its default.
    #[must_use]
    pub const fn with_adapted_payload(mut self, adapted_payload: AdaptedPayload) -> Self {
        self.adapted_payload = adapted_payload;
        self
    }

    /// Set the `Prefer: ngsi-ld=` version the payload is adapted to, if any.
    #[must_use]
    pub const fn with_requested_version(mut self, requested_version: Option<SpecVersion>) -> Self {
        self.requested_version = requested_version;
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

    /// Attach the payload to render, producing a runnable [`ReadyResponse`].
    ///
    /// The only way to reach [`ReadyResponse::render`], so a response body
    /// cannot be produced without a payload.
    #[must_use]
    pub const fn with_payload(self, payload: ResponsePayload) -> ReadyResponse<'a> {
        ReadyResponse {
            renderer: self,
            payload,
            targets: TargetDecoding::Pending,
        }
    }

    /// Attach the typed entities to render, in the representation set by
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
}

impl ReadyResponse<'_> {
    /// Run the pipeline.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::Serialization`] when the `GeoJSON` envelope cannot
    /// be turned into a JSON value.
    pub fn render(self) -> Result<RenderedOutput, RenderError> {
        let ReadyResponse { renderer, payload, targets } = self;
        let geo_json_shape = match renderer.media_type {
            ResponseMediaType::Json | ResponseMediaType::JsonLd => None,
            ResponseMediaType::GeoJson => payload.geo_json_shape(),
        };

        let carries_relationships = payload.carries_relationships();
        let mut value = payload.into_value();

        // Decode percent-encoded Relationship/ListRelationship URIs before any
        // GeoJSON wrapping, so the policy is media-type-independent: the walk
        // runs while the payload is still a plain entity object, and the Feature
        // envelope is built afterwards from the already-decoded value.
        if carries_relationships && targets == TargetDecoding::Pending {
            decode_percent_encoded_uris(&mut value);
        }

        // Version adaptation drives the 203 status, so the flag comes from
        // whether the renderer actually adapted -- a conformant-or-newer target
        // short-circuits and leaves the payload authoritative (200).
        let version_adaptation = match renderer.requested_version {
            Some(target) => {
                let (adapted, adaptation) = VersionRenderer::new(target)
                    .with_format(renderer.format)
                    .with_adapted_payload(renderer.adapted_payload)
                    .with_payload(value)
                    .render();
                value = adapted;
                adaptation
            }
            None => VersionAdaptation::None,
        };

        // Exactly one carrier holds the context, so the body value and the
        // `Link` target are read from the one variant the caller chose.
        let (context_value, context_link) = match renderer.context {
            ResponseContext::Body(context) => (Some(context), None),
            ResponseContext::Link(target) => (None, Some(target)),
            ResponseContext::Absent => (None, None),
        };

        value = if let Some(shape) = geo_json_shape {
            GeoJsonRenderer::new(value)
                .with_context(context_value)
                .with_geometry_selection(renderer.geometry_selection)
                .with_shape(shape)
                .render()?
        } else {
            inject_context(&mut value, context_value);
            value
        };

        Ok(RenderedOutput {
            value,
            head: ResponseHead {
                context_link,
                media_type: renderer.media_type,
                version_adaptation,
            },
        })
    }
}

/// Insert an `@context` member into a rendered payload.
///
/// A collection is an array of entities, and each one carries its own
/// `@context`: an array of entities is not a JSON-LD document, so there is no
/// enclosing object the member could go on instead.
fn inject_context(value: &mut Value, context_value: Option<Value>) {
    let Some(context_value) = context_value else { return };

    match value {
        Value::Object(object) => {
            object.insert("@context".to_string(), context_value);
        }
        Value::Array(items) => {
            // Formatted once and shared through an `Arc` so the value is not
            // rebuilt per entity; `Value` owns its data, so the insert itself
            // still copies.
            let shared = Arc::new(context_value);
            for item in items.iter_mut() {
                if let Some(object) = item.as_object_mut() {
                    object.insert("@context".to_string(), (*shared).clone());
                }
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        context::ResponseContext,
        response::{ResponsePayload, ResponseRenderer},
        version::adaptation::VersionAdaptation,
    };
    use andromeda_http_headers::media_type::ResponseMediaType;
    use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;
    use iri_rs::IriBuf;
    use serde_json::{Value, json};

    /// A context URL as the absolute [`IriBuf`] a `Link` header names.
    fn iri(url: &str) -> IriBuf {
        IriBuf::new(url.to_string()).expect("a valid IRI")
    }

    /// A context the body embeds.
    fn embedded() -> ResponseContext {
        ResponseContext::Body(json!("https://example.org/ctx"))
    }

    /// A context the `Link` header advertises.
    fn linked() -> ResponseContext {
        ResponseContext::Link(iri("https://example.org/ctx"))
    }

    fn an_entity() -> Value {
        json!({"id": "urn:x", "type": "T", "name": {"type": "Property", "value": "test"}})
    }

    #[test]
    fn a_linked_context_is_not_injected_and_is_advertised() {
        let rendered = ResponseRenderer::new(ResponseMediaType::Json, linked())
            .with_payload(ResponsePayload::Single(an_entity()))
            .render()
            .expect("render");
        assert!(rendered.value.get("@context").is_none());
        pretty_assertions::assert_eq!(rendered.head.context_link, Some(iri("https://example.org/ctx")));
        pretty_assertions::assert_eq!(rendered.head.media_type, ResponseMediaType::Json);
    }

    #[test]
    fn an_embedded_context_is_injected_and_not_advertised() {
        let rendered = ResponseRenderer::new(ResponseMediaType::JsonLd, embedded())
            .with_payload(ResponsePayload::Single(json!({"id": "urn:x", "type": "T"})))
            .render()
            .expect("render");
        pretty_assertions::assert_eq!(rendered.value["@context"], json!("https://example.org/ctx"));
        pretty_assertions::assert_eq!(rendered.head.context_link, None);
        pretty_assertions::assert_eq!(rendered.head.media_type, ResponseMediaType::JsonLd);
    }

    #[test]
    fn render_collection_ld_json_injects_context_per_item() {
        let values = vec![json!({"id": "urn:x", "type": "T"}), json!({"id": "urn:y", "type": "T"})];
        let rendered = ResponseRenderer::new(ResponseMediaType::JsonLd, embedded())
            .with_payload(ResponsePayload::Collection(values))
            .render()
            .expect("render");
        let items = rendered.value.as_array().expect("array");
        pretty_assertions::assert_eq!(items.len(), 2);
        assert!(items[0].get("@context").is_some());
        assert!(items[1].get("@context").is_some());
    }

    #[test]
    fn render_batch_injects_context_for_ld_json() {
        let rendered = ResponseRenderer::new(ResponseMediaType::JsonLd, embedded())
            .with_payload(ResponsePayload::Batch(json!({"success": ["urn:x"], "errors": []})))
            .render()
            .expect("render");
        assert!(rendered.value.get("@context").is_some());
    }

    /// Clause 6.3.6: a partial-success body names only fully qualified names,
    /// "as there is no context present", so it carries a context nowhere.
    #[test]
    fn an_absent_context_is_neither_injected_nor_advertised() {
        let rendered = ResponseRenderer::new(ResponseMediaType::Json, ResponseContext::Absent)
            .with_payload(ResponsePayload::Batch(json!({"success": ["urn:x"], "errors": []})))
            .render()
            .expect("render");
        assert!(rendered.value.get("@context").is_none());
        pretty_assertions::assert_eq!(rendered.head.context_link, None);
    }

    #[test]
    fn a_batch_result_is_never_wrapped_in_a_geojson_envelope() {
        // Clause 4.5.16 wraps entities; a BatchOperationResult is not one, and
        // wrapping it would put `success` and `errors` inside a Feature's
        // properties where no client looks for them.
        let rendered = ResponseRenderer::new(ResponseMediaType::GeoJson, linked())
            .with_payload(ResponsePayload::Batch(json!({"success": ["urn:x"], "errors": []})))
            .render()
            .expect("render");
        assert!(rendered.value.get("type").is_none(), "a Feature envelope was added: {}", rendered.value);
        pretty_assertions::assert_eq!(rendered.value["success"], json!(["urn:x"]));
    }

    #[test]
    fn a_single_entity_renders_as_a_geojson_feature_and_a_collection_as_a_feature_collection() {
        let single = ResponseRenderer::new(ResponseMediaType::GeoJson, embedded())
            .with_payload(ResponsePayload::Single(an_entity()))
            .render()
            .expect("render");
        pretty_assertions::assert_eq!(single.value["type"], "Feature");

        let collection = ResponseRenderer::new(ResponseMediaType::GeoJson, embedded())
            .with_payload(ResponsePayload::Collection(vec![an_entity()]))
            .render()
            .expect("render");
        pretty_assertions::assert_eq!(collection.value["type"], "FeatureCollection");
    }

    /// Clause 4.5.16: a `GeoJSON` envelope holds the context it is given, and
    /// with `Prefer: body=json` the context is advertised instead.
    #[test]
    fn a_geojson_response_embeds_or_advertises_the_context_it_is_given() {
        let embedded_feature = ResponseRenderer::new(ResponseMediaType::GeoJson, embedded())
            .with_payload(ResponsePayload::Single(an_entity()))
            .render()
            .expect("render");
        assert!(embedded_feature.value.get("@context").is_some());
        pretty_assertions::assert_eq!(embedded_feature.head.context_link, None);

        let advertised = ResponseRenderer::new(ResponseMediaType::GeoJson, linked())
            .with_payload(ResponsePayload::Single(an_entity()))
            .render()
            .expect("render");
        assert!(advertised.value.get("@context").is_none());
        pretty_assertions::assert_eq!(advertised.head.context_link, Some(iri("https://example.org/ctx")));
    }

    #[test]
    fn relationship_and_list_relationship_uris_are_decoded_under_every_media_type() {
        // The decode runs over the plain entity before any GeoJSON wrapping, so
        // it is media-type-independent: a Feature carries the same decoded URIs
        // its JSON counterpart does. Both a Relationship's `object` and a
        // ListRelationship's `objectList` entries are decoded.
        let payload = || {
            json!({
                "id": "urn:x", "type": "T",
                "ref": {"type": "Relationship", "object": "urn%3Ay"},
                "refs": {"type": "ListRelationship", "objectList": ["urn%3Aa", "urn%3Ab"]}
            })
        };

        let json_rendered = ResponseRenderer::new(ResponseMediaType::Json, linked())
            .with_payload(ResponsePayload::Single(payload()))
            .render()
            .expect("render");
        pretty_assertions::assert_eq!(json_rendered.value["ref"]["object"], "urn:y");
        pretty_assertions::assert_eq!(json_rendered.value["refs"]["objectList"], json!(["urn:a", "urn:b"]));

        let geo_rendered = ResponseRenderer::new(ResponseMediaType::GeoJson, embedded())
            .with_payload(ResponsePayload::Single(payload()))
            .render()
            .expect("render");
        pretty_assertions::assert_eq!(geo_rendered.value["properties"]["ref"]["object"], "urn:y");
        pretty_assertions::assert_eq!(geo_rendered.value["properties"]["refs"]["objectList"], json!(["urn:a", "urn:b"]));
    }

    #[test]
    fn a_conformant_target_version_yields_no_adaptation() {
        // Clause 4.3.6.8: `Prefer: ngsi-ld=1.9` names the current version, so
        // nothing is adapted and the response stays 200. Setting the flag from
        // the mere presence of a requested version made this a spurious 203.
        let rendered = ResponseRenderer::new(ResponseMediaType::Json, linked())
            .with_requested_version(Some(SpecVersion::CONFORMANT))
            .with_payload(ResponsePayload::Single(
                json!({"id": "urn:x", "type": "T", "expiresAt": "2025-01-01T00:00:00Z"}),
            ))
            .render()
            .expect("render");
        pretty_assertions::assert_eq!(rendered.head.version_adaptation, VersionAdaptation::Unchanged);
    }

    #[test]
    fn a_batch_result_keeps_its_relationship_shaped_members_undecoded() {
        // Nothing in a batch result is an attribute, so the walk is skipped
        // outright rather than run over a payload that cannot match.
        let rendered = ResponseRenderer::new(ResponseMediaType::Json, linked())
            .with_payload(ResponsePayload::Batch(
                json!({"success": [], "errors": [{"entityId": "urn%3Ay", "type": "Relationship", "object": "urn%3Az"}]}),
            ))
            .render()
            .expect("render");
        pretty_assertions::assert_eq!(rendered.value["errors"][0]["object"], "urn%3Az");
    }

    #[test]
    fn render_single_version_adapted_flag() {
        let rendered = ResponseRenderer::new(ResponseMediaType::Json, linked())
            .with_requested_version(Some(SpecVersion::V1_4))
            .with_payload(ResponsePayload::Single(json!({"id": "urn:x", "type": "T"})))
            .render()
            .expect("render");
        pretty_assertions::assert_eq!(rendered.head.version_adaptation, VersionAdaptation::Applied { version: SpecVersion::V1_4 });
    }

    #[test]
    fn render_single_no_version_adaptation_by_default() {
        let rendered = ResponseRenderer::new(ResponseMediaType::Json, linked())
            .with_payload(ResponsePayload::Single(json!({"id": "urn:x", "type": "T"})))
            .render()
            .expect("render");
        pretty_assertions::assert_eq!(rendered.head.version_adaptation, VersionAdaptation::None);
    }
}
