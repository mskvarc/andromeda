//! GeoJSON Feature and FeatureCollection mapping for NGSI-LD entities.
//!
//! Handles the conversion of NGSI-LD entity JSON objects into GeoJSON Features
//! and FeatureCollections per NGSI-LD spec clause 6.3.8.  Uses the `geojson`
//! crate for type-safe construction; `@context` is injected post-serialization
//! because it is an NGSI-LD extension absent from the GeoJSON spec.

use crate::error::{RenderedShape, RepresentationError};
use andromeda_ngsi_ld_vocabulary::{compact::properties as compact_properties, expanded::properties as expanded_properties};
use geojson::{Feature, FeatureCollection, feature::Id};
use serde_json::{Map, Value};

/// Output shape selector for [`GeoJsonRenderer`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GeoJsonShape {
    /// Render a single entity as a GeoJSON Feature.
    #[default]
    Feature,
    /// Render an array of entities as a GeoJSON FeatureCollection.
    FeatureCollection,
}

/// Renders NGSI-LD JSON objects into GeoJSON Features or FeatureCollections.
#[derive(Debug)]
pub struct GeoJsonRenderer<'a> {
    /// The entity or entity array to wrap.
    payload: Value,
    /// The `@context` to inject into the envelope, if it goes in the body.
    context_value: Option<Value>,
    /// The attribute the geometry is taken from; `location` when absent.
    geometry_property: Option<&'a str>,
    /// Which envelope to build.
    shape: GeoJsonShape,
}

impl<'a> GeoJsonRenderer<'a> {
    /// Create a new renderer for the given entity/collection JSON payload.
    pub const fn new(payload: Value) -> GeoJsonRenderer<'a> {
        GeoJsonRenderer {
            payload,
            context_value: None,
            geometry_property: None,
            shape: GeoJsonShape::Feature,
        }
    }

    /// Set the JSON-LD `@context` value to inject into the output.
    pub fn with_context(mut self, ctx: Option<Value>) -> Self {
        self.context_value = ctx;
        self
    }

    /// Set a custom geometry property name (e.g. `observationSpace`).
    pub const fn with_geometry_property(mut self, prop: Option<&'a str>) -> Self {
        self.geometry_property = prop;
        self
    }

    /// Set the output shape (Feature for single entity, FeatureCollection for an array).
    pub const fn with_shape(mut self, shape: GeoJsonShape) -> Self {
        self.shape = shape;
        self
    }
}

impl<'a> GeoJsonRenderer<'a> {
    /// Wrap the payload in the clause 6.3.8 GeoJSON envelope.
    ///
    /// # Errors
    ///
    /// Returns [`RepresentationError::Serialization`] when the assembled Feature or
    /// FeatureCollection cannot be turned into a JSON value.
    pub fn render(self) -> Result<Value, RepresentationError> {
        let mut value = match self.shape {
            GeoJsonShape::Feature => {
                let feature = entity_to_feature(&self.payload, self.geometry_property);
                serde_json::to_value(feature).map_err(|error| RepresentationError::serialization(RenderedShape::GeoJsonFeature, error))?
            }
            GeoJsonShape::FeatureCollection => {
                let features = match &self.payload {
                    Value::Array(items) => items.iter().map(|e| entity_to_feature(e, self.geometry_property)).collect(),
                    Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_) => Vec::new(),
                };
                let collection = FeatureCollection {
                    bbox: None,
                    features,
                    foreign_members: None,
                };
                serde_json::to_value(collection).map_err(|error| RepresentationError::serialization(RenderedShape::GeoJsonFeatureCollection, error))?
            }
        };
        if let Some(ctx) = self.context_value
            && let Some(obj) = value.as_object_mut()
        {
            obj.insert("@context".to_string(), ctx);
        }
        Ok(value)
    }
}

/// Convert a single NGSI-LD entity JSON object into a GeoJSON [`Feature`].
fn entity_to_feature(entity: &Value, geometry_property: Option<&str>) -> Feature {
    let Some(obj) = entity.as_object() else {
        return Feature {
            bbox: None,
            geometry: None,
            id: None,
            properties: Some(Map::new()),
            foreign_members: None,
        };
    };

    let id = obj.get("id").and_then(Value::as_str).map(|s| Id::String(s.to_string()));

    let geometry = extract_geometry(obj, geometry_property);

    // Properties: keep all attributes including the geometry source.
    // Remove `id` (promoted to Feature.id); keep `type` per NGSI-LD clause 6.3.8.
    let mut props = obj.clone();
    props.remove("id");

    Feature {
        bbox: None,
        geometry,
        id,
        properties: Some(props),
        foreign_members: None,
    }
}

/// Extract a GeoJSON [`geojson::Geometry`] from an entity object using the
/// configured geometry property name.
fn extract_geometry(obj: &Map<String, Value>, geometry_property: Option<&str>) -> Option<geojson::Geometry> {
    let geom_key = geometry_property.unwrap_or(compact_properties::LOCATION);
    let location_keys: &[&str] = &[compact_properties::LOCATION, expanded_properties::LOCATION.as_str()];
    let keys: &[&str] = if geom_key == compact_properties::LOCATION {
        location_keys
    } else {
        // Single custom key -- use a local slice
        &[geom_key]
    };

    let geojson_value = keys.iter().find_map(|key| obj.get(*key)).and_then(|loc| {
        // Normalized: {"type": "GeoProperty", "value": {<geojson>}}
        if let Some(val) = loc.get("value") {
            return Some(val.clone());
        }
        // Simplified/keyValues: location is the GeoJSON directly
        if loc.get("type").and_then(Value::as_str).is_some_and(|t| {
            matches!(
                t,
                "Point" | "MultiPoint" | "LineString" | "MultiLineString" | "Polygon" | "MultiPolygon" | "GeometryCollection"
            )
        }) {
            return Some(loc.clone());
        }
        None
    })?;

    serde_json::from_value(geojson_value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn geojson_collection_contains_features() {
        let entities = json!([
            {
                "id": "urn:ngsi-ld:Building:1",
                "type": "Building",
                "location": {
                    "type": "GeoProperty",
                    "value": {
                        "type": "Point",
                        "coordinates": [13.3986, 52.5547]
                    }
                },
                "name": {"type": "Property", "value": "Eiffel Tower"}
            }
        ]);

        let geo = GeoJsonRenderer::new(entities)
            .with_shape(GeoJsonShape::FeatureCollection)
            .render()
            .expect("GeoJSON collection");
        pretty_assertions::assert_eq!(geo["type"], "FeatureCollection");
        let features = geo["features"].as_array().expect("features array");
        pretty_assertions::assert_eq!(features.len(), 1);
        pretty_assertions::assert_eq!(features[0]["geometry"]["type"], "Point");
        // type is in properties per NGSI-LD spec
        pretty_assertions::assert_eq!(features[0]["properties"]["type"], "Building");
        // id is NOT in properties (promoted to Feature.id)
        assert!(features[0]["properties"].get("id").is_none());
        // location IS in properties per NGSI-LD spec clause 6.3.8
        assert!(features[0]["properties"]["location"].is_object());
    }

    #[test]
    fn null_geometry_when_no_location() {
        let entity = json!({"id": "urn:x", "type": "T", "name": {"type": "Property", "value": "foo"}});
        let geo = GeoJsonRenderer::new(entity).render().expect("GeoJSON entity");
        pretty_assertions::assert_eq!(geo["type"], "Feature");
        assert!(geo["geometry"].is_null());
    }

    #[test]
    fn custom_geometry_property() {
        let entity = json!({
            "id": "urn:x", "type": "T",
            "observationSpace": {
                "type": "GeoProperty",
                "value": {"type": "Polygon", "coordinates": [[[0,0],[1,0],[1,1],[0,1],[0,0]]]}
            }
        });
        let geo = GeoJsonRenderer::new(entity)
            .with_geometry_property(Some("observationSpace"))
            .render()
            .expect("GeoJSON entity");
        pretty_assertions::assert_eq!(geo["geometry"]["type"], "Polygon");
    }

    #[test]
    fn simplified_entity_direct_geojson() {
        let entity = json!({
            "id": "urn:x", "type": "T",
            "location": {"type": "Point", "coordinates": [1.0, 2.0]}
        });
        let geo = GeoJsonRenderer::new(entity).render().expect("GeoJSON entity");
        pretty_assertions::assert_eq!(geo["geometry"]["type"], "Point");
        pretty_assertions::assert_eq!(geo["geometry"]["coordinates"], json!([1.0, 2.0]));
    }

    #[test]
    fn feature_id_matches_entity_id() {
        let entity = json!({"id": "urn:ngsi-ld:Entity:42", "type": "T"});
        let geo = GeoJsonRenderer::new(entity).render().expect("GeoJSON entity");
        pretty_assertions::assert_eq!(geo["id"], "urn:ngsi-ld:Entity:42");
    }

    #[test]
    fn empty_array_produces_empty_collection() {
        let geo = GeoJsonRenderer::new(json!([]))
            .with_shape(GeoJsonShape::FeatureCollection)
            .render()
            .expect("GeoJSON collection");
        pretty_assertions::assert_eq!(geo["type"], "FeatureCollection");
        pretty_assertions::assert_eq!(geo["features"].as_array().expect("features").len(), 0);
    }

    #[test]
    fn context_injected_into_feature() {
        let entity = json!({"id": "urn:x", "type": "T"});
        let ctx = json!("https://example.org/ctx");
        let geo = GeoJsonRenderer::new(entity).with_context(Some(ctx.clone())).render().expect("GeoJSON entity");
        pretty_assertions::assert_eq!(geo["@context"], ctx);
    }

    #[test]
    fn context_injected_into_feature_collection() {
        let entities = json!([{"id": "urn:x", "type": "T"}]);
        let ctx = json!("https://example.org/ctx");
        let geo = GeoJsonRenderer::new(entities)
            .with_context(Some(ctx.clone()))
            .with_shape(GeoJsonShape::FeatureCollection)
            .render()
            .expect("GeoJSON collection");
        pretty_assertions::assert_eq!(geo["@context"], ctx);
    }

    #[test]
    fn non_object_entity_produces_feature_with_null_geometry() {
        let geo = GeoJsonRenderer::new(json!("bad")).render().expect("GeoJSON entity");
        pretty_assertions::assert_eq!(geo["type"], "Feature");
        assert!(geo["geometry"].is_null());
    }
}
