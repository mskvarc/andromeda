//! Central response rendering pipeline for NGSI-LD payloads.
//!
//! [`ResponseRenderer`] encapsulates all post-domain transformations that must
//! be applied before a payload is serialized to the wire:
//!
//! - Percent-encoded URI decoding in Relationship `object` fields
//! - Version adaptation for older NGSI-LD spec versions (`Prefer: ngsi-ld=<version>`)
//! - `@context` body injection for `application/ld+json` responses
//! - GeoJSON Feature / FeatureCollection wrapping for `application/geo+json` responses
//!
//! API handlers configure the renderer via builder methods and call
//! [`Renderer::render`]. The resulting [`RenderedOutput`] is passed directly
//! to the response type for header construction and serialization.

use crate::{
    core::error::AndromedaError,
    http::{
        extractor::prefer::PreferHeader,
        model::{preference::GeoJsonBodyPreference, response_format::ResponseFormat},
    },
    jsonld::ContextSpec,
    renderer::{
        ContextLocation,
        Renderer,
        VersionAdaptation,
        entity::decode_percent_encoded_uris,
        geojson::{GeoJsonRenderer, GeoJsonShape},
        version::VersionRenderer,
    },
};
use serde_json::Value;
use std::{mem::replace, sync::Arc};

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

/// Builder for the response rendering pipeline.
///
/// Configured by handlers based on Accept header, JSON-LD context, client
/// preferences, and optional geometry property for GeoJSON conversion.
#[derive(Debug)]
pub struct ResponseRenderer {
    format: ResponseFormat,
    context: ContextSpec,
    prefer: PreferHeader,
    geometry_property: Option<String>,
    payload: ResponsePayload,
}

/// Output of the rendering pipeline, ready for the response type to serialize.
#[derive(Debug)]
pub struct RenderedOutput {
    /// The fully-rendered JSON value (with @context injected if applicable).
    pub value: Value,
    /// The JSON-LD context spec for Link header construction.
    pub context: ContextSpec,
    /// The Content-Type string for this response.
    pub content_type: &'static str,
    /// Where the JSON-LD `@context` should appear: in the `Link` header or in the body.
    pub context_location: ContextLocation,
    /// Whether version adaptation was applied (triggers 203 status).
    pub version_adaptation: VersionAdaptation,
}

impl ResponseRenderer {
    /// Create a new renderer with the given format, context, and preferences.
    ///
    /// The payload defaults to [`ResponsePayload::Single`] with a JSON null;
    /// call [`ResponseRenderer::with_payload`] to supply the real payload.
    pub const fn new(format: ResponseFormat, context: ContextSpec, prefer: PreferHeader) -> ResponseRenderer {
        ResponseRenderer {
            format,
            context,
            prefer,
            geometry_property: None,
            payload: ResponsePayload::Single(Value::Null),
        }
    }

    /// Set the geometry property for GeoJSON conversion.
    pub fn with_geometry_property(mut self, geometry_property: Option<String>) -> Self {
        self.geometry_property = geometry_property;
        self
    }

    /// Set the payload to render.
    pub fn with_payload(mut self, payload: ResponsePayload) -> Self {
        self.payload = payload;
        self
    }
}

impl Renderer for ResponseRenderer {
    type Output = RenderedOutput;

    fn render(mut self) -> Result<RenderedOutput, AndromedaError> {
        let payload = replace(&mut self.payload, ResponsePayload::Single(Value::Null));
        match payload {
            ResponsePayload::Single(value) => self.render_single(value),
            ResponsePayload::Collection(values) => self.render_collection(values),
            ResponsePayload::Batch(value) => self.render_batch(value),
        }
    }
}

impl ResponseRenderer {
    /// Render a single entity/object payload.
    ///
    /// Applies: URI decoding (Json/JsonLd), version adaptation, @context injection
    /// or GeoJSON wrapping depending on the response format.
    fn render_single(self, mut value: Value) -> Result<RenderedOutput, AndromedaError> {
        let version_adaptation = if self.prefer.requested_version.is_some() {
            VersionAdaptation::Applied
        } else {
            VersionAdaptation::None
        };
        let context_value = self.context.for_presentation().to_context_value();

        let context_location = match self.format {
            ResponseFormat::Json | ResponseFormat::JsonLd => {
                if self.format.include_context_in_body() {
                    ContextLocation::Body
                } else {
                    ContextLocation::LinkHeader
                }
            }
            ResponseFormat::GeoJson => {
                if self.prefer.geo_json_body == GeoJsonBodyPreference::Json {
                    ContextLocation::LinkHeader
                } else {
                    ContextLocation::Body
                }
            }
        };

        match self.format {
            ResponseFormat::Json | ResponseFormat::JsonLd => {
                decode_percent_encoded_uris(&mut value);
                if let Some(target) = self.prefer.requested_version {
                    value = VersionRenderer::new(target).with_payload(value).render()?;
                }
                if self.format.include_context_in_body()
                    && let (Some(obj), Some(cv)) = (value.as_object_mut(), context_value)
                {
                    obj.insert("@context".to_string(), format_context(cv));
                }
            }
            ResponseFormat::GeoJson => {
                if let Some(target) = self.prefer.requested_version {
                    value = VersionRenderer::new(target).with_payload(value).render()?;
                }
                let geojson_context = if self.prefer.geo_json_body == GeoJsonBodyPreference::Json {
                    None
                } else {
                    context_value
                };
                value = GeoJsonRenderer::new(value)
                    .with_context(geojson_context)
                    .with_geometry_property(self.geometry_property.as_deref())
                    .with_shape(GeoJsonShape::Feature)
                    .render()?;
            }
        }

        Ok(RenderedOutput {
            value,
            context: self.context,
            content_type: self.format.content_type_str(),
            context_location,
            version_adaptation,
        })
    }

    /// Render a collection of entities/objects.
    ///
    /// Applies: URI decoding (Json/JsonLd), version adaptation, per-item @context
    /// injection or GeoJSON FeatureCollection wrapping depending on the format.
    fn render_collection(self, values: Vec<Value>) -> Result<RenderedOutput, AndromedaError> {
        let version_adaptation = if self.prefer.requested_version.is_some() {
            VersionAdaptation::Applied
        } else {
            VersionAdaptation::None
        };
        let context_value = self.context.for_presentation().to_context_value();

        let context_location = match self.format {
            ResponseFormat::Json | ResponseFormat::JsonLd => {
                if self.format.include_context_in_body() {
                    ContextLocation::Body
                } else {
                    ContextLocation::LinkHeader
                }
            }
            ResponseFormat::GeoJson => {
                if self.prefer.geo_json_body == GeoJsonBodyPreference::Json {
                    ContextLocation::LinkHeader
                } else {
                    ContextLocation::Body
                }
            }
        };

        let mut array_value = Value::Array(values);

        match self.format {
            ResponseFormat::Json | ResponseFormat::JsonLd => {
                decode_percent_encoded_uris(&mut array_value);
                if let Some(target) = self.prefer.requested_version {
                    array_value = VersionRenderer::new(target).with_payload(array_value).render()?;
                }
                if self.format.include_context_in_body()
                    && let Some(cv) = context_value
                    && let Value::Array(ref mut items) = array_value
                {
                    // Format once, share across items via Arc so the injected
                    // value is not recloned per entity. `Value::Array` and
                    // `Value::Object` internally hold owned data, so cloning
                    // the Arc payload is still required at insert time, but
                    // `format_context` runs once.
                    let formatted = Arc::new(format_context(cv));
                    for item in items.iter_mut() {
                        if let Some(obj) = item.as_object_mut() {
                            obj.insert("@context".to_string(), (*formatted).clone());
                        }
                    }
                }
            }
            ResponseFormat::GeoJson => {
                if let Some(target) = self.prefer.requested_version {
                    array_value = VersionRenderer::new(target).with_payload(array_value).render()?;
                }
                let geojson_context = if self.prefer.geo_json_body == GeoJsonBodyPreference::Json {
                    None
                } else {
                    context_value
                };
                array_value = GeoJsonRenderer::new(array_value)
                    .with_context(geojson_context)
                    .with_geometry_property(self.geometry_property.as_deref())
                    .with_shape(GeoJsonShape::FeatureCollection)
                    .render()?;
            }
        }

        Ok(RenderedOutput {
            value: array_value,
            context: self.context,
            content_type: self.format.content_type_str(),
            context_location,
            version_adaptation,
        })
    }

    /// Render a batch operation result.
    ///
    /// Applies: version adaptation, @context injection. No URI decoding
    /// (batch results don't contain entity Relationship objects) and no
    /// GeoJSON wrapping.
    fn render_batch(self, mut value: Value) -> Result<RenderedOutput, AndromedaError> {
        let version_adaptation = if self.prefer.requested_version.is_some() {
            VersionAdaptation::Applied
        } else {
            VersionAdaptation::None
        };
        let context_value = self.context.for_presentation().to_context_value();
        let context_location = if self.format.include_context_in_body() {
            ContextLocation::Body
        } else {
            ContextLocation::LinkHeader
        };

        if let Some(target) = self.prefer.requested_version {
            value = VersionRenderer::new(target).with_payload(value).render()?;
        }
        if self.format.include_context_in_body()
            && let (Some(obj), Some(cv)) = (value.as_object_mut(), context_value)
        {
            obj.insert("@context".to_string(), format_context(cv));
        }

        Ok(RenderedOutput {
            value,
            context: self.context,
            content_type: self.format.content_type_str(),
            context_location,
            version_adaptation,
        })
    }
}

/// Format a `@context` value for `ld+json` responses.
///
/// Per NGSI-LD conformance: a single context URL stays as a string,
/// multiple contexts are serialized as an array.
fn format_context(ctx: Value) -> Value {
    match ctx {
        Value::Array(mut arr) if arr.len() == 1 => arr.swap_remove(0),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::extractor::prefer::PreferHeader;
    use serde_json::json;

    fn default_prefer() -> PreferHeader {
        PreferHeader::default()
    }

    #[test]
    fn render_single_json_does_not_inject_context() {
        let ctx = ContextSpec::from_links(vec!["https://example.org/ctx".to_string()]);
        let value = json!({"id": "urn:x", "type": "T", "name": {"type": "Property", "value": "test"}});
        let rendered = ResponseRenderer::new(ResponseFormat::Json, ctx, default_prefer())
            .with_payload(ResponsePayload::Single(value))
            .render()
            .expect("render");
        assert!(rendered.value.get("@context").is_none());
        pretty_assertions::assert_eq!(rendered.context_location, ContextLocation::LinkHeader);
        pretty_assertions::assert_eq!(rendered.content_type, "application/json");
    }

    #[test]
    fn render_single_ld_json_injects_context() {
        let ctx = ContextSpec::from_links(vec!["https://example.org/ctx".to_string()]);
        let value = json!({"id": "urn:x", "type": "T"});
        let rendered = ResponseRenderer::new(ResponseFormat::JsonLd, ctx, default_prefer())
            .with_payload(ResponsePayload::Single(value))
            .render()
            .expect("render");
        assert!(rendered.value.get("@context").is_some());
        pretty_assertions::assert_eq!(rendered.context_location, ContextLocation::Body);
        pretty_assertions::assert_eq!(rendered.content_type, "application/ld+json");
    }

    #[test]
    fn render_collection_ld_json_injects_context_per_item() {
        let ctx = ContextSpec::from_links(vec!["https://example.org/ctx".to_string()]);
        let values = vec![json!({"id": "urn:x", "type": "T"}), json!({"id": "urn:y", "type": "T"})];
        let rendered = ResponseRenderer::new(ResponseFormat::JsonLd, ctx, default_prefer())
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
        let ctx = ContextSpec::from_links(vec!["https://example.org/ctx".to_string()]);
        let value = json!({"success": ["urn:x"], "errors": []});
        let rendered = ResponseRenderer::new(ResponseFormat::JsonLd, ctx, default_prefer())
            .with_payload(ResponsePayload::Batch(value))
            .render()
            .expect("render");
        assert!(rendered.value.get("@context").is_some());
    }

    #[test]
    fn render_single_version_adapted_flag() {
        use crate::http::model::version::NgsiLdVersion;
        let ctx = ContextSpec::from_links(vec!["https://example.org/ctx".to_string()]);
        let prefer = PreferHeader {
            requested_version: Some(NgsiLdVersion::V1_4),
            ..PreferHeader::default()
        };
        let value = json!({"id": "urn:x", "type": "T"});
        let rendered = ResponseRenderer::new(ResponseFormat::Json, ctx, prefer)
            .with_payload(ResponsePayload::Single(value))
            .render()
            .expect("render");
        pretty_assertions::assert_eq!(rendered.version_adaptation, VersionAdaptation::Applied);
    }

    #[test]
    fn render_single_no_version_adaptation_by_default() {
        let ctx = ContextSpec::from_links(vec!["https://example.org/ctx".to_string()]);
        let value = json!({"id": "urn:x", "type": "T"});
        let rendered = ResponseRenderer::new(ResponseFormat::Json, ctx, default_prefer())
            .with_payload(ResponsePayload::Single(value))
            .render()
            .expect("render");
        pretty_assertions::assert_eq!(rendered.version_adaptation, VersionAdaptation::None);
    }

    #[test]
    fn format_context_unwraps_single_element_array() {
        pretty_assertions::assert_eq!(format_context(json!(["https://example.org/ctx"])), json!("https://example.org/ctx"));
    }

    #[test]
    fn format_context_preserves_multi_element_array() {
        let input = json!(["https://a.org", "https://b.org"]);
        pretty_assertions::assert_eq!(format_context(input.clone()), input);
    }

    #[test]
    fn format_context_passes_through_string() {
        pretty_assertions::assert_eq!(format_context(json!("https://example.org/ctx")), json!("https://example.org/ctx"));
    }
}
