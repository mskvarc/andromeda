//! The response pipeline: everything that happens to a payload after the
//! broker has decided what to answer and before it is written to the wire.
//!
//! Four transformations, in this order, none of them optional in the sense
//! that a caller could reorder them:
//!
//! 1. percent-decoding of the URIs a Relationship points at,
//! 2. version adaptation for a client that asked for an older spec version
//!    (`Prefer: ngsi-ld=<version>`, clause 4.3.6.8),
//! 3. `@context` injection for a body that carries it (clause 5.5.7), and
//! 4. the GeoJSON Feature or FeatureCollection envelope (clause 4.5.16).
//!
//! Which of them run is a function of the media type and the payload shape,
//! and of nothing else — which is why there is one pipeline here rather than
//! one per shape. The differences are declared as [`ResponsePayload`]'s three
//! variants and read once.

use crate::{
    context::{ContextLocation, ResponseContext},
    error::RenderError,
    geojson::{GeoJsonRenderer, GeoJsonShape},
    uri_decoding::decode_percent_encoded_uris,
    version::{VersionAdaptation, VersionRenderer},
};
use andromeda_ngsi_ld_http_headers::{geo_json_body::GeoJsonBodyPreference, media_type::ResponseMediaType, version::NgsiLdVersion};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use iri_rs::IriRefBuf;
use serde_json::Value;
use std::sync::Arc;

/// Payload fed to [`ResponseRenderer`]: the three NGSI-LD response shapes.
///
/// The three variants are *not* interchangeable: batch results skip
/// percent-encoded URI decoding and never participate in GeoJSON wrapping,
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

    /// The GeoJSON envelope this shape takes, if it takes one.
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
/// the resolved `@context`, the client's `Prefer` tokens and — for GeoJSON —
/// the attribute the geometry is taken from. It cannot be rendered on its own:
/// only [`ResponseRenderer::with_payload`] produces a [`ReadyResponse`], and
/// only that carries [`ReadyResponse::render`]. A caller therefore cannot forget
/// the payload — the type system will not let it — which is why the payload is
/// not an optional field defaulted to a JSON null.
#[derive(Debug)]
pub struct ResponseRenderer<'a> {
    /// The negotiated media type, which decides the envelope and the carrier.
    media_type: ResponseMediaType,
    /// The resolved `@context`, in both the spellings the carriers need.
    context: ResponseContext,
    /// The `Prefer: body=` token, which moves a GeoJSON `@context` to the headers.
    geo_json_body: GeoJsonBodyPreference,
    /// The `Prefer: ngsi-ld=` version to adapt the payload to, if any.
    requested_version: Option<NgsiLdVersion>,
    /// The attribute a GeoJSON geometry is taken from.
    geometry_property: Option<&'a AttributeName>,
}

/// A fully-configured renderer with its payload attached, ready to run.
///
/// The only value that carries [`ReadyResponse::render`]; it exists solely
/// because a [`ResponseRenderer`] gained its payload, so a response body can
/// never be rendered without one.
#[derive(Debug)]
pub struct ReadyResponse<'a> {
    /// The configuration the payload was attached to.
    renderer: ResponseRenderer<'a>,
    /// What is being answered.
    payload: ResponsePayload,
}

/// Output of the rendering pipeline, ready for the response type to serialize.
#[derive(Debug)]
pub struct RenderedOutput {
    /// The fully-rendered JSON value, with `@context` injected if applicable.
    pub value: Value,
    /// The context URLs a `Link` header should advertise.
    pub context_links: Vec<IriRefBuf>,
    /// The media type this response is written as.
    pub media_type: ResponseMediaType,
    /// Where the JSON-LD `@context` ended up.
    pub context_location: ContextLocation,
    /// Whether version adaptation was applied, which makes the status 203.
    pub version_adaptation: VersionAdaptation,
}

impl<'a> ResponseRenderer<'a> {
    /// Create a renderer for the given media type and resolved `@context`.
    pub const fn new(media_type: ResponseMediaType, context: ResponseContext) -> ResponseRenderer<'a> {
        ResponseRenderer {
            media_type,
            context,
            geo_json_body: GeoJsonBodyPreference::LdJson,
            requested_version: None,
            geometry_property: None,
        }
    }

    /// Set the `Prefer: body=` token, which decides where a GeoJSON response
    /// carries its `@context` (clause 4.5.16).
    pub const fn with_geo_json_body(mut self, geo_json_body: GeoJsonBodyPreference) -> Self {
        self.geo_json_body = geo_json_body;
        self
    }

    /// Set the `Prefer: ngsi-ld=` version the payload is adapted to, if any.
    pub const fn with_requested_version(mut self, requested_version: Option<NgsiLdVersion>) -> Self {
        self.requested_version = requested_version;
        self
    }

    /// Set the attribute a GeoJSON geometry is taken from (clause 4.5.16).
    pub const fn with_geometry_property(mut self, geometry_property: Option<&'a AttributeName>) -> Self {
        self.geometry_property = geometry_property;
        self
    }

    /// Attach the payload to render, producing a runnable [`ReadyResponse`].
    ///
    /// The only way to reach [`ReadyResponse::render`], so a response body
    /// cannot be produced without a payload.
    pub const fn with_payload(self, payload: ResponsePayload) -> ReadyResponse<'a> {
        ReadyResponse { renderer: self, payload }
    }

    /// Where this response carries its `@context`.
    ///
    /// `application/ld+json` carries it in the body and `application/json`
    /// cannot, so it goes in the `Link` header. GeoJSON is the exception: the
    /// envelope can hold an `@context` member, and clause 4.5.16 lets the client
    /// ask for it not to with `Prefer: body=json`.
    const fn context_location(&self, geo_json_shape: Option<GeoJsonShape>) -> ContextLocation {
        match geo_json_shape {
            Some(_) => match self.geo_json_body {
                GeoJsonBodyPreference::Json => ContextLocation::LinkHeader,
                GeoJsonBodyPreference::LdJson => ContextLocation::Body,
            },
            None => {
                if self.media_type.include_context_in_body() {
                    ContextLocation::Body
                } else {
                    ContextLocation::LinkHeader
                }
            }
        }
    }
}

impl ReadyResponse<'_> {
    /// Run the pipeline.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::Serialization`] when the GeoJSON envelope cannot
    /// be turned into a JSON value.
    pub fn render(self) -> Result<RenderedOutput, RenderError> {
        let ReadyResponse { mut renderer, payload } = self;
        let geo_json_shape = match renderer.media_type {
            ResponseMediaType::Json | ResponseMediaType::JsonLd => None,
            ResponseMediaType::GeoJson => payload.geo_json_shape(),
        };
        let context_location = renderer.context_location(geo_json_shape);

        let carries_relationships = payload.carries_relationships();
        let mut value = payload.into_value();

        // Decode percent-encoded Relationship/ListRelationship URIs before any
        // GeoJSON wrapping, so the policy is media-type-independent: the walk
        // runs while the payload is still a plain entity object, and the Feature
        // envelope is built afterwards from the already-decoded value.
        if carries_relationships {
            decode_percent_encoded_uris(&mut value);
        }

        // Version adaptation drives the 203 status, so the flag comes from
        // whether the renderer actually adapted -- a conformant-or-newer target
        // short-circuits and leaves the payload authoritative (200).
        let version_adaptation = match renderer.requested_version {
            Some(target) => {
                let (adapted, adaptation) = VersionRenderer::new(target).with_payload(value).render();
                value = adapted;
                adaptation
            }
            None => VersionAdaptation::None,
        };

        let context_value = match context_location {
            ContextLocation::Body => renderer.context.take_body_value(),
            ContextLocation::LinkHeader => None,
        };

        value = match geo_json_shape {
            Some(shape) => GeoJsonRenderer::new(value)
                .with_context(context_value)
                .with_geometry_property(renderer.geometry_property)
                .with_shape(shape)
                .render()?,
            None => {
                inject_context(&mut value, context_value);
                value
            }
        };

        Ok(RenderedOutput {
            value,
            context_links: renderer.context.link_urls().to_vec(),
            media_type: renderer.media_type,
            context_location,
            version_adaptation,
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
        context::{ContextLocation, ResponseContext},
        response::{ResponsePayload, ResponseRenderer},
        version::VersionAdaptation,
    };
    use andromeda_ngsi_ld_http_headers::{geo_json_body::GeoJsonBodyPreference, media_type::ResponseMediaType, version::NgsiLdVersion};
    use iri_rs::IriRefBuf;
    use serde_json::{Value, json};

    /// A context URL as the [`IriRefBuf`] a `Link` header carries.
    fn iri_ref(url: &str) -> IriRefBuf {
        IriRefBuf::new(url.to_string()).expect("a valid IRI reference")
    }

    fn a_context() -> ResponseContext {
        ResponseContext::new(Some(json!("https://example.org/ctx")), vec![iri_ref("https://example.org/ctx")])
    }

    fn an_entity() -> Value {
        json!({"id": "urn:x", "type": "T", "name": {"type": "Property", "value": "test"}})
    }

    #[test]
    fn render_single_json_does_not_inject_context() {
        let rendered = ResponseRenderer::new(ResponseMediaType::Json, a_context())
            .with_payload(ResponsePayload::Single(an_entity()))
            .render()
            .expect("render");
        assert!(rendered.value.get("@context").is_none());
        pretty_assertions::assert_eq!(rendered.context_location, ContextLocation::LinkHeader);
        pretty_assertions::assert_eq!(rendered.media_type, ResponseMediaType::Json);
    }

    #[test]
    fn render_single_ld_json_injects_context() {
        let rendered = ResponseRenderer::new(ResponseMediaType::JsonLd, a_context())
            .with_payload(ResponsePayload::Single(json!({"id": "urn:x", "type": "T"})))
            .render()
            .expect("render");
        pretty_assertions::assert_eq!(rendered.value["@context"], json!("https://example.org/ctx"));
        pretty_assertions::assert_eq!(rendered.context_location, ContextLocation::Body);
        pretty_assertions::assert_eq!(rendered.media_type, ResponseMediaType::JsonLd);
    }

    #[test]
    fn render_collection_ld_json_injects_context_per_item() {
        let values = vec![json!({"id": "urn:x", "type": "T"}), json!({"id": "urn:y", "type": "T"})];
        let rendered = ResponseRenderer::new(ResponseMediaType::JsonLd, a_context())
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
        let rendered = ResponseRenderer::new(ResponseMediaType::JsonLd, a_context())
            .with_payload(ResponsePayload::Batch(json!({"success": ["urn:x"], "errors": []})))
            .render()
            .expect("render");
        assert!(rendered.value.get("@context").is_some());
    }

    #[test]
    fn a_batch_result_is_never_wrapped_in_a_geojson_envelope() {
        // Clause 4.5.16 wraps entities; a BatchOperationResult is not one, and
        // wrapping it would put `success` and `errors` inside a Feature's
        // properties where no client looks for them.
        let rendered = ResponseRenderer::new(ResponseMediaType::GeoJson, a_context())
            .with_payload(ResponsePayload::Batch(json!({"success": ["urn:x"], "errors": []})))
            .render()
            .expect("render");
        assert!(rendered.value.get("type").is_none(), "a Feature envelope was added: {}", rendered.value);
        pretty_assertions::assert_eq!(rendered.value["success"], json!(["urn:x"]));
    }

    #[test]
    fn a_single_entity_renders_as_a_geojson_feature_and_a_collection_as_a_feature_collection() {
        let single = ResponseRenderer::new(ResponseMediaType::GeoJson, a_context())
            .with_payload(ResponsePayload::Single(an_entity()))
            .render()
            .expect("render");
        pretty_assertions::assert_eq!(single.value["type"], "Feature");

        let collection = ResponseRenderer::new(ResponseMediaType::GeoJson, a_context())
            .with_payload(ResponsePayload::Collection(vec![an_entity()]))
            .render()
            .expect("render");
        pretty_assertions::assert_eq!(collection.value["type"], "FeatureCollection");
    }

    #[test]
    fn a_geojson_response_moves_its_context_to_the_link_header_when_body_json_is_preferred() {
        let embedded = ResponseRenderer::new(ResponseMediaType::GeoJson, a_context())
            .with_geo_json_body(GeoJsonBodyPreference::LdJson)
            .with_payload(ResponsePayload::Single(an_entity()))
            .render()
            .expect("render");
        pretty_assertions::assert_eq!(embedded.context_location, ContextLocation::Body);
        assert!(embedded.value.get("@context").is_some());

        let advertised = ResponseRenderer::new(ResponseMediaType::GeoJson, a_context())
            .with_geo_json_body(GeoJsonBodyPreference::Json)
            .with_payload(ResponsePayload::Single(an_entity()))
            .render()
            .expect("render");
        pretty_assertions::assert_eq!(advertised.context_location, ContextLocation::LinkHeader);
        assert!(advertised.value.get("@context").is_none());
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

        let json_rendered = ResponseRenderer::new(ResponseMediaType::Json, a_context())
            .with_payload(ResponsePayload::Single(payload()))
            .render()
            .expect("render");
        pretty_assertions::assert_eq!(json_rendered.value["ref"]["object"], "urn:y");
        pretty_assertions::assert_eq!(json_rendered.value["refs"]["objectList"], json!(["urn:a", "urn:b"]));

        let geo_rendered = ResponseRenderer::new(ResponseMediaType::GeoJson, a_context())
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
        let rendered = ResponseRenderer::new(ResponseMediaType::Json, a_context())
            .with_requested_version(Some(NgsiLdVersion::CONFORMANT))
            .with_payload(ResponsePayload::Single(
                json!({"id": "urn:x", "type": "T", "expiresAt": "2025-01-01T00:00:00Z"}),
            ))
            .render()
            .expect("render");
        pretty_assertions::assert_eq!(rendered.version_adaptation, VersionAdaptation::None);
    }

    #[test]
    fn a_batch_result_keeps_its_relationship_shaped_members_undecoded() {
        // Nothing in a batch result is an attribute, so the walk is skipped
        // outright rather than run over a payload that cannot match.
        let rendered = ResponseRenderer::new(ResponseMediaType::Json, a_context())
            .with_payload(ResponsePayload::Batch(
                json!({"success": [], "errors": [{"entityId": "urn%3Ay", "type": "Relationship", "object": "urn%3Az"}]}),
            ))
            .render()
            .expect("render");
        pretty_assertions::assert_eq!(rendered.value["errors"][0]["object"], "urn%3Az");
    }

    #[test]
    fn render_single_version_adapted_flag() {
        let rendered = ResponseRenderer::new(ResponseMediaType::Json, a_context())
            .with_requested_version(Some(NgsiLdVersion::V1_4))
            .with_payload(ResponsePayload::Single(json!({"id": "urn:x", "type": "T"})))
            .render()
            .expect("render");
        pretty_assertions::assert_eq!(rendered.version_adaptation, VersionAdaptation::Applied);
    }

    #[test]
    fn render_single_no_version_adaptation_by_default() {
        let rendered = ResponseRenderer::new(ResponseMediaType::Json, a_context())
            .with_payload(ResponsePayload::Single(json!({"id": "urn:x", "type": "T"})))
            .render()
            .expect("render");
        pretty_assertions::assert_eq!(rendered.version_adaptation, VersionAdaptation::None);
    }

    #[test]
    fn the_link_urls_survive_every_media_type_including_the_ones_that_embed_the_context() {
        for media_type in [ResponseMediaType::Json, ResponseMediaType::JsonLd, ResponseMediaType::GeoJson] {
            let rendered = ResponseRenderer::new(media_type, a_context())
                .with_payload(ResponsePayload::Single(an_entity()))
                .render()
                .expect("render");
            pretty_assertions::assert_eq!(
                rendered.context_links,
                vec![iri_ref("https://example.org/ctx")],
                "link URLs lost for {media_type:?}"
            );
        }
    }
}
