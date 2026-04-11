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
//! API handlers configure the renderer via builder methods and call one of the
//! `render_*` methods. The resulting [`RenderedOutput`] is passed directly to
//! the response type for header construction and serialization.

use crate::{
    core::error::AndromedaError,
    http::{
        extractor::prefer::PreferHeader,
        model::{preference::GeoJsonBodyPreference, response_format::ResponseFormat},
    },
    jsonld::ContextSpec,
    renderer::{ContextLocation, VersionAdaptation, entity::decode_percent_encoded_uris, geojson::GeoJsonRenderer, version::VersionRenderer},
};
use serde_json::Value;

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
    pub const fn new(format: ResponseFormat, context: ContextSpec, prefer: PreferHeader) -> ResponseRenderer {
        ResponseRenderer {
            format,
            context,
            prefer,
            geometry_property: None,
        }
    }

    /// Set the geometry property for GeoJSON conversion.
    pub fn with_geometry_property(mut self, geometry_property: Option<String>) -> Self {
        self.geometry_property = geometry_property;
        self
    }

    /// Render a single entity/object payload.
    ///
    /// Applies: URI decoding (Json/JsonLd), version adaptation, @context injection
    /// or GeoJSON wrapping depending on the response format.
    pub fn render_single(self, mut value: Value) -> Result<RenderedOutput, AndromedaError> {
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
                    VersionRenderer::new(target).render(&mut value);
                }
                if self.format.include_context_in_body()
                    && let (Some(obj), Some(cv)) = (value.as_object_mut(), context_value)
                {
                    obj.insert("@context".to_string(), format_context(cv));
                }
            }
            ResponseFormat::GeoJson => {
                if let Some(target) = self.prefer.requested_version {
                    VersionRenderer::new(target).render(&mut value);
                }
                let geojson_context = if self.prefer.geo_json_body == GeoJsonBodyPreference::Json {
                    None
                } else {
                    context_value
                };
                value = GeoJsonRenderer::new(value)
                    .with_context(geojson_context)
                    .with_geometry_property(self.geometry_property.as_deref())
                    .render_entity()?;
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
    pub fn render_collection(self, values: Vec<Value>) -> Result<RenderedOutput, AndromedaError> {
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
                    VersionRenderer::new(target).render(&mut array_value);
                }
                if self.format.include_context_in_body()
                    && let Value::Array(ref mut items) = array_value
                {
                    for item in items.iter_mut() {
                        if let (Some(obj), Some(cv)) = (item.as_object_mut(), context_value.clone()) {
                            obj.insert("@context".to_string(), format_context(cv));
                        }
                    }
                }
            }
            ResponseFormat::GeoJson => {
                if let Some(target) = self.prefer.requested_version {
                    VersionRenderer::new(target).render(&mut array_value);
                }
                let geojson_context = if self.prefer.geo_json_body == GeoJsonBodyPreference::Json {
                    None
                } else {
                    context_value
                };
                array_value = GeoJsonRenderer::new(array_value)
                    .with_context(geojson_context)
                    .with_geometry_property(self.geometry_property.as_deref())
                    .render_collection()?;
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
    pub fn render_batch(self, mut value: Value) -> Result<RenderedOutput, AndromedaError> {
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
            VersionRenderer::new(target).render(&mut value);
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
            .render_single(value)
            .expect("render");
        assert!(rendered.value.get("@context").is_none());
        assert_eq!(rendered.context_location, ContextLocation::LinkHeader);
        assert_eq!(rendered.content_type, "application/json");
    }

    #[test]
    fn render_single_ld_json_injects_context() {
        let ctx = ContextSpec::from_links(vec!["https://example.org/ctx".to_string()]);
        let value = json!({"id": "urn:x", "type": "T"});
        let rendered = ResponseRenderer::new(ResponseFormat::JsonLd, ctx, default_prefer())
            .render_single(value)
            .expect("render");
        assert!(rendered.value.get("@context").is_some());
        assert_eq!(rendered.context_location, ContextLocation::Body);
        assert_eq!(rendered.content_type, "application/ld+json");
    }

    #[test]
    fn render_collection_ld_json_injects_context_per_item() {
        let ctx = ContextSpec::from_links(vec!["https://example.org/ctx".to_string()]);
        let values = vec![json!({"id": "urn:x", "type": "T"}), json!({"id": "urn:y", "type": "T"})];
        let rendered = ResponseRenderer::new(ResponseFormat::JsonLd, ctx, default_prefer())
            .render_collection(values)
            .expect("render");
        let items = rendered.value.as_array().expect("array");
        assert_eq!(items.len(), 2);
        assert!(items[0].get("@context").is_some());
        assert!(items[1].get("@context").is_some());
    }

    #[test]
    fn render_batch_injects_context_for_ld_json() {
        let ctx = ContextSpec::from_links(vec!["https://example.org/ctx".to_string()]);
        let value = json!({"success": ["urn:x"], "errors": []});
        let rendered = ResponseRenderer::new(ResponseFormat::JsonLd, ctx, default_prefer())
            .render_batch(value)
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
        let rendered = ResponseRenderer::new(ResponseFormat::Json, ctx, prefer).render_single(value).expect("render");
        assert_eq!(rendered.version_adaptation, VersionAdaptation::Applied);
    }

    #[test]
    fn render_single_no_version_adaptation_by_default() {
        let ctx = ContextSpec::from_links(vec!["https://example.org/ctx".to_string()]);
        let value = json!({"id": "urn:x", "type": "T"});
        let rendered = ResponseRenderer::new(ResponseFormat::Json, ctx, default_prefer())
            .render_single(value)
            .expect("render");
        assert_eq!(rendered.version_adaptation, VersionAdaptation::None);
    }

    #[test]
    fn format_context_unwraps_single_element_array() {
        assert_eq!(format_context(json!(["https://example.org/ctx"])), json!("https://example.org/ctx"));
    }

    #[test]
    fn format_context_preserves_multi_element_array() {
        let input = json!(["https://a.org", "https://b.org"]);
        assert_eq!(format_context(input.clone()), input);
    }

    #[test]
    fn format_context_passes_through_string() {
        assert_eq!(format_context(json!("https://example.org/ctx")), json!("https://example.org/ctx"));
    }
}
