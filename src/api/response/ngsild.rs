//! NGSI-LD response types with automatic content negotiation.
//!
//! These types handle the formatting of responses based on Accept header:
//! - `application/json`: @context in Link header, not in body
//! - `application/ld+json`: @context as URL in response body

use crate::{
    api::{extractor::prefer::PreferHeader, response::version_compat::adapt_payload_for_version},
    header::{GeoJsonBodyPreference, PreferenceApplied, ResponseFormat, insert_jsonld_contexts},
    jsonld::ContextSpec,
    service::{context_source_forwarding::NgsiLdWarning, entity::decode_percent_encoded_uris},
};
use axum::{
    Json,
    http::{HeaderMap, HeaderValue, StatusCode, header::CONTENT_TYPE},
    response::{IntoResponse, Response},
};
use headers::HeaderMapExt;
use serde::Serialize;
use serde_json::{Map, Value};

/// Format a @context value for ld+json responses.
/// Per NGSI-LD conformance: a single context URL stays as a string,
/// multiple contexts are serialized as an array.
fn format_context(ctx: Value) -> Value {
    match ctx {
        Value::Array(ref arr) if arr.len() == 1 => arr[0].clone(),
        other => other,
    }
}

/// NGSI-LD response wrapper that handles content negotiation automatically.
///
/// # Example
///
/// ```ignore
/// NgsiLdResponse::ok(entity)
///     .with_context(context)
///     .with_format(accept.format())
/// ```
pub struct NgsiLdResponse<T> {
    status: StatusCode,
    data: T,
    context: Option<ContextSpec>,
    format: ResponseFormat,
    geometry_property: Option<String>,
    location: Option<String>,
    entity_map_uri: Option<String>,
    snapshot_uri: Option<String>,
    warnings: Vec<NgsiLdWarning>,
    prefer: PreferHeader,
}

impl<T> NgsiLdResponse<T> {
    /// Create a response with a specific status code.
    pub fn new(status: StatusCode, data: T) -> Self {
        Self {
            status,
            data,
            context: None,
            format: ResponseFormat::default(),
            geometry_property: None,
            location: None,
            entity_map_uri: None,
            snapshot_uri: None,
            warnings: Vec::new(),
            prefer: PreferHeader::default(),
        }
    }

    /// Create a 200 OK response.
    pub fn ok(data: T) -> Self {
        Self::new(StatusCode::OK, data)
    }

    /// Create a response with custom status code.
    #[allow(dead_code)]
    pub const fn with_status(mut self, status: StatusCode) -> Self {
        self.status = status;
        self
    }

    /// Set the JSON-LD context for this response.
    pub fn with_context(mut self, context: ContextSpec) -> Self {
        self.context = Some(context);
        self
    }

    /// Set the response format (from Accept header).
    pub const fn with_format(mut self, format: ResponseFormat) -> Self {
        self.format = format;
        self
    }

    /// Set the geometry property for GeoJSON conversion.
    pub fn with_geometry_property(mut self, geometry_property: Option<String>) -> Self {
        self.geometry_property = geometry_property;
        self
    }

    /// Set the Location header.
    pub fn with_location(mut self, location: String) -> Self {
        self.location = Some(location);
        self
    }

    /// Set the NGSILD-EntityMap header URI.
    pub fn with_entity_map_uri(mut self, uri: String) -> Self {
        self.entity_map_uri = Some(uri);
        self
    }

    /// Set NGSILD-Warning headers from distributed forwarding (clause 6.3.17).
    pub fn with_warnings(mut self, warnings: Vec<NgsiLdWarning>) -> Self {
        self.warnings = warnings;
        self
    }

    /// Set the Prefer header preferences (clause 6.3.6).
    pub const fn with_prefer(mut self, prefer: PreferHeader) -> Self {
        self.prefer = prefer;
        self
    }

    /// Set the NGSILD-Snapshot header for snapshot-scoped responses (clause 6.3.22).
    pub fn with_snapshot_uri(mut self, uri: String) -> Self {
        self.snapshot_uri = Some(uri);
        self
    }
}

impl<T: Serialize> IntoResponse for NgsiLdResponse<T> {
    fn into_response(self) -> Response {
        let context_spec = self.context.map_or_else(ContextSpec::none, |c| c.for_presentation());
        let context_value = context_spec.to_context_value();

        // Determine effective status: 203 if version preference is present
        let status = if self.prefer.requested_version.is_some() {
            StatusCode::NON_AUTHORITATIVE_INFORMATION
        } else {
            self.status
        };

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static(self.format.content_type()));

        // Insert Preference-Applied header when version preference is present
        if self.prefer.requested_version.is_some() {
            headers.typed_insert(PreferenceApplied::version_conformance());
        }

        if let Some(ref loc) = self.location
            && let Ok(val) = HeaderValue::from_str(loc)
        {
            headers.insert("Location", val);
        }

        if let Some(ref uri) = self.entity_map_uri
            && let Ok(val) = HeaderValue::from_str(uri)
        {
            headers.insert("NGSILD-EntityMap", val);
        }

        if let Some(ref snap_uri) = self.snapshot_uri
            && let Ok(val) = HeaderValue::from_str(snap_uri)
        {
            headers.insert("NGSILD-Snapshot", val);
        }

        for warning in &self.warnings {
            if let Ok(val) = HeaderValue::from_str(&warning.header_value()) {
                headers.append("NGSILD-Warning", val);
            }
        }

        match self.format {
            ResponseFormat::Json => {
                // application/json: @context in Link header, not in body
                let urls = context_spec.to_link_headers();
                insert_jsonld_contexts(&mut headers, &urls);

                match serde_json::to_value(&self.data) {
                    Ok(mut value) => {
                        decode_percent_encoded_uris(&mut value);
                        if let Some(target) = self.prefer.requested_version {
                            adapt_payload_for_version(&mut value, target);
                        }
                        (status, headers, Json(value)).into_response()
                    }
                    Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
                }
            }
            ResponseFormat::JsonLd => {
                // application/ld+json: @context as URL array in body
                match serde_json::to_value(&self.data) {
                    Ok(mut value) => {
                        decode_percent_encoded_uris(&mut value);
                        if let Some(target) = self.prefer.requested_version {
                            adapt_payload_for_version(&mut value, target);
                        }
                        if let (Some(obj), Some(context_value)) = (value.as_object_mut(), context_value) {
                            obj.insert("@context".to_string(), format_context(context_value));
                        }
                        (status, headers, Json(value)).into_response()
                    }
                    Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
                }
            }
            ResponseFormat::GeoJson => match serde_json::to_value(&self.data) {
                Ok(mut value) => {
                    if let Some(target) = self.prefer.requested_version {
                        adapt_payload_for_version(&mut value, target);
                    }
                    // GeoJSON body preference: body=json means @context in Link header
                    let geojson_context = if self.prefer.geo_json_body == GeoJsonBodyPreference::Json {
                        let urls = context_spec.to_link_headers();
                        insert_jsonld_contexts(&mut headers, &urls);
                        None
                    } else {
                        context_value
                    };
                    let geojson = entity_to_feature(value, geojson_context, self.geometry_property.as_deref());
                    (status, headers, Json(geojson)).into_response()
                }
                Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            },
        }
    }
}

/// NGSI-LD response for arrays/collections with proper content negotiation.
///
/// Handles additional NGSI-LD collection response requirements like
/// `NGSILD-Results-Count` header for pagination.
pub struct NgsiLdCollectionResponse<T> {
    status: StatusCode,
    data: Vec<T>,
    context: Option<ContextSpec>,
    format: ResponseFormat,
    total_count: Option<usize>,
    geometry_property: Option<String>,
    content_range: Option<String>,
    entity_map_uri: Option<String>,
    snapshot_uri: Option<String>,
    warnings: Vec<NgsiLdWarning>,
    prefer: PreferHeader,
}

impl<T> NgsiLdCollectionResponse<T> {
    /// Create a 200 OK response for a collection.
    pub fn ok(data: Vec<T>) -> Self {
        Self {
            status: StatusCode::OK,
            data,
            context: None,
            format: ResponseFormat::default(),
            total_count: None,
            geometry_property: None,
            content_range: None,
            entity_map_uri: None,
            snapshot_uri: None,
            warnings: Vec::new(),
            prefer: PreferHeader::default(),
        }
    }

    /// Set the JSON-LD context for this response.
    pub fn with_context(mut self, context: ContextSpec) -> Self {
        self.context = Some(context);
        self
    }

    /// Set the response format (from Accept header).
    pub const fn with_format(mut self, format: ResponseFormat) -> Self {
        self.format = format;
        self
    }

    /// Set the total count for pagination (NGSILD-Results-Count header).
    #[allow(dead_code)]
    pub const fn with_total_count(mut self, count: usize) -> Self {
        self.total_count = Some(count);
        self
    }

    /// Override the HTTP status code.
    pub const fn with_status(mut self, status: StatusCode) -> Self {
        self.status = status;
        self
    }

    /// Set the geometry property for GeoJSON conversion.
    pub fn with_geometry_property(mut self, geometry_property: Option<String>) -> Self {
        self.geometry_property = geometry_property;
        self
    }

    /// Set the Content-Range header value for temporal pagination.
    pub fn with_content_range(mut self, content_range: String) -> Self {
        self.content_range = Some(content_range);
        self
    }

    /// Set the NGSILD-EntityMap header URI.
    pub fn with_entity_map_uri(mut self, uri: String) -> Self {
        self.entity_map_uri = Some(uri);
        self
    }

    /// Set NGSILD-Warning headers from distributed forwarding (clause 6.3.17).
    pub fn with_warnings(mut self, warnings: Vec<NgsiLdWarning>) -> Self {
        self.warnings = warnings;
        self
    }

    /// Set the Prefer header preferences (clause 6.3.6).
    pub const fn with_prefer(mut self, prefer: PreferHeader) -> Self {
        self.prefer = prefer;
        self
    }

    /// Set the NGSILD-Snapshot header for snapshot-scoped responses (clause 6.3.22).
    pub fn with_snapshot_uri(mut self, uri: String) -> Self {
        self.snapshot_uri = Some(uri);
        self
    }
}

impl<T: Serialize> IntoResponse for NgsiLdCollectionResponse<T> {
    fn into_response(self) -> Response {
        let context_spec = self.context.map_or_else(ContextSpec::none, |c| c.for_presentation());
        let context_value = context_spec.to_context_value();

        // Determine effective status: 203 if version preference is present
        let status = if self.prefer.requested_version.is_some() {
            StatusCode::NON_AUTHORITATIVE_INFORMATION
        } else {
            self.status
        };

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static(self.format.content_type()));

        // Insert Preference-Applied header when version preference is present
        if self.prefer.requested_version.is_some() {
            headers.typed_insert(PreferenceApplied::version_conformance());
        }

        // Add NGSILD-Results-Count if provided
        if let Some(count) = self.total_count
            && let Ok(val) = HeaderValue::from_str(&count.to_string())
        {
            headers.insert("NGSILD-Results-Count", val);
        }

        // Add Content-Range if provided (temporal pagination)
        if let Some(ref cr) = self.content_range
            && let Ok(val) = HeaderValue::from_str(cr)
        {
            headers.insert("Content-Range", val);
        }

        // Add NGSILD-EntityMap header if provided
        if let Some(ref uri) = self.entity_map_uri
            && let Ok(val) = HeaderValue::from_str(uri)
        {
            headers.insert("NGSILD-EntityMap", val);
        }

        if let Some(ref snap_uri) = self.snapshot_uri
            && let Ok(val) = HeaderValue::from_str(snap_uri)
        {
            headers.insert("NGSILD-Snapshot", val);
        }

        for warning in &self.warnings {
            if let Ok(val) = HeaderValue::from_str(&warning.header_value()) {
                headers.append("NGSILD-Warning", val);
            }
        }

        // Add Link header for application/json
        if self.format == ResponseFormat::Json {
            let urls = context_spec.to_link_headers();
            insert_jsonld_contexts(&mut headers, &urls);
        }

        match self.format {
            ResponseFormat::Json => {
                // Return array without @context
                match serde_json::to_value(&self.data) {
                    Ok(mut value) => {
                        decode_percent_encoded_uris(&mut value);
                        if let Some(target) = self.prefer.requested_version {
                            adapt_payload_for_version(&mut value, target);
                        }
                        (status, headers, Json(value)).into_response()
                    }
                    Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
                }
            }
            ResponseFormat::JsonLd => {
                // Add @context array to each item
                match serde_json::to_value(&self.data) {
                    Ok(serde_json::Value::Array(mut items)) => {
                        for item in &mut items {
                            decode_percent_encoded_uris(item);
                            if let Some(target) = self.prefer.requested_version {
                                adapt_payload_for_version(item, target);
                            }
                            if let (Some(obj), Some(context_value)) = (item.as_object_mut(), context_value.clone()) {
                                obj.insert("@context".to_string(), format_context(context_value));
                            }
                        }
                        (status, headers, Json(items)).into_response()
                    }
                    Ok(other) => (status, headers, Json(other)).into_response(),
                    Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
                }
            }
            ResponseFormat::GeoJson => match serde_json::to_value(&self.data) {
                Ok(mut value) => {
                    if let Some(target) = self.prefer.requested_version {
                        adapt_payload_for_version(&mut value, target);
                    }
                    // GeoJSON body preference: body=json means @context in Link header
                    let geojson_context = if self.prefer.geo_json_body == GeoJsonBodyPreference::Json {
                        let urls = context_spec.to_link_headers();
                        insert_jsonld_contexts(&mut headers, &urls);
                        None
                    } else {
                        context_value
                    };
                    let geojson = entities_to_feature_collection(value, geojson_context, self.geometry_property.as_deref());
                    (status, headers, Json(geojson)).into_response()
                }
                Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            },
        }
    }
}

fn entity_to_feature(entity: Value, context_value: Option<Value>, geometry_property: Option<&str>) -> Value {
    let feature = entity_object_to_feature_with_geometry_property(&entity, geometry_property);
    if let Some(ctx) = context_value {
        match feature {
            Value::Object(mut obj) => {
                obj.insert("@context".to_string(), ctx);
                Value::Object(obj)
            }
            other => other,
        }
    } else {
        feature
    }
}

fn entities_to_feature_collection(entities: Value, context_value: Option<Value>, geometry_property: Option<&str>) -> Value {
    let features = match entities {
        Value::Array(items) => items
            .iter()
            .map(|e| entity_object_to_feature_with_geometry_property(e, geometry_property))
            .collect::<Vec<Value>>(),
        _ => Vec::new(),
    };

    let mut collection = Map::new();
    collection.insert("type".to_string(), Value::String("FeatureCollection".to_string()));
    collection.insert("features".to_string(), Value::Array(features));
    if let Some(ctx) = context_value {
        collection.insert("@context".to_string(), ctx);
    }
    Value::Object(collection)
}

fn entity_object_to_feature_with_geometry_property(entity: &Value, geometry_property: Option<&str>) -> Value {
    let Some(obj) = entity.as_object() else {
        return Value::Object(Map::from_iter([
            ("type".to_string(), Value::String("Feature".to_string())),
            ("geometry".to_string(), Value::Null),
            ("properties".to_string(), Value::Object(Map::new())),
        ]));
    };

    let id = obj.get("id").cloned().unwrap_or(Value::Null);

    // Determine which attribute to use as geometry
    let geom_key = geometry_property.unwrap_or("location");
    let geometry_keys: Vec<&str> = if geom_key == "location" {
        vec!["location", "https://uri.etsi.org/ngsi-ld/location"]
    } else {
        vec![geom_key]
    };

    let geometry = geometry_keys
        .iter()
        .find_map(|key| obj.get(*key))
        .and_then(|loc| {
            // Normalized: {"type": "GeoProperty", "value": {<geojson>}}
            if let Some(val) = loc.get("value") {
                return Some(val.clone());
            }
            // Simplified/keyValues: location is the GeoJSON directly
            if loc.get("type").and_then(|t| t.as_str()).is_some_and(|t| {
                matches!(
                    t,
                    "Point" | "MultiPoint" | "LineString" | "MultiLineString" | "Polygon" | "MultiPolygon" | "GeometryCollection"
                )
            }) {
                return Some(loc.clone());
            }
            None
        })
        .unwrap_or(Value::Null);

    // Properties: keep all attributes including the geometry source
    let mut props = obj.clone();
    props.remove("id");
    // Keep "type" and geometry attribute in properties per NGSI-LD spec clause 6.3.8

    let mut feature = Map::new();
    feature.insert("id".to_string(), id);
    feature.insert("type".to_string(), Value::String("Feature".to_string()));
    feature.insert("geometry".to_string(), geometry);
    feature.insert("properties".to_string(), Value::Object(props));
    Value::Object(feature)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geojson_collection_contains_features() {
        let entities = serde_json::json!([
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

        let geo = entities_to_feature_collection(entities, None, None);
        assert_eq!(geo.get("type").and_then(|v| v.as_str()), Some("FeatureCollection"));
        assert_eq!(geo.get("features").and_then(|v| v.as_array()).map(|a| a.len()), Some(1));
        assert_eq!(
            geo.get("features")
                .and_then(|v| v.get(0))
                .and_then(|v| v.get("geometry"))
                .and_then(|v| v.get("type"))
                .and_then(|v| v.as_str()),
            Some("Point")
        );
        // Verify type is in properties per NGSI-LD spec
        assert_eq!(
            geo.get("features")
                .and_then(|v| v.get(0))
                .and_then(|v| v.get("properties"))
                .and_then(|v| v.get("type"))
                .and_then(|v| v.as_str()),
            Some("Building")
        );
        // Verify id is NOT in properties (it's at feature level)
        assert!(
            geo.get("features")
                .and_then(|v| v.get(0))
                .and_then(|v| v.get("properties"))
                .and_then(|v| v.get("id"))
                .is_none()
        );
        // Verify location IS in properties per NGSI-LD spec clause 6.3.8
        assert!(
            geo.get("features")
                .and_then(|v| v.get(0))
                .and_then(|v| v.get("properties"))
                .and_then(|v| v.get("location"))
                .is_some()
        );
    }

    #[test]
    fn ngsild_response_includes_warning_headers_per_clause_6_3_17() {
        use axum::response::IntoResponse;
        let warnings = vec![
            NgsiLdWarning::MiscellaneousWarning("timeout reaching CS".to_string()),
            NgsiLdWarning::RevalidationFailed("invalid payload".to_string()),
        ];
        let response = NgsiLdResponse::ok(serde_json::json!({"id": "urn:a", "type": "T"}))
            .with_warnings(warnings)
            .into_response();
        let warning_values: Vec<&str> = response.headers().get_all("NGSILD-Warning").iter().filter_map(|v| v.to_str().ok()).collect();
        assert_eq!(warning_values.len(), 2);
        assert_eq!(warning_values[0], "199 - \"timeout reaching CS\"");
        assert_eq!(warning_values[1], "111 - \"invalid payload\"");
    }

    #[test]
    fn ngsild_collection_response_includes_warning_headers_per_clause_6_3_17() {
        use axum::response::IntoResponse;
        let warnings = vec![NgsiLdWarning::MiscellaneousPersistentWarning("CS returned HTTP 500".to_string())];
        let response = NgsiLdCollectionResponse::ok(vec![serde_json::json!({"id": "urn:a", "type": "T"})])
            .with_warnings(warnings)
            .into_response();
        let warning_values: Vec<&str> = response.headers().get_all("NGSILD-Warning").iter().filter_map(|v| v.to_str().ok()).collect();
        assert_eq!(warning_values.len(), 1);
        assert_eq!(warning_values[0], "299 - \"CS returned HTTP 500\"");
    }

    #[test]
    fn ngsild_response_without_warnings_has_no_warning_header() {
        use axum::response::IntoResponse;
        let response = NgsiLdResponse::ok(serde_json::json!({"id": "urn:a", "type": "T"})).into_response();
        let warning_values: Vec<&str> = response.headers().get_all("NGSILD-Warning").iter().filter_map(|v| v.to_str().ok()).collect();
        assert!(warning_values.is_empty());
    }

    #[test]
    fn geojson_body_json_preference_uses_link_header() {
        use axum::response::IntoResponse;

        let prefer = PreferHeader {
            geo_json_body: GeoJsonBodyPreference::Json,
            requested_version: None,
        };
        let entity = serde_json::json!({
            "id": "urn:ngsi-ld:Building:1",
            "type": "Building",
            "location": {
                "type": "GeoProperty",
                "value": {
                    "type": "Point",
                    "coordinates": [13.3986, 52.5547]
                }
            }
        });
        let response = NgsiLdResponse::ok(entity)
            .with_format(ResponseFormat::GeoJson)
            .with_context(ContextSpec::from_links(vec!["https://example.com/ctx.jsonld".to_string()]))
            .with_prefer(prefer)
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        // Link header should be present (context moved there)
        assert!(response.headers().get("link").is_some());
    }

    #[test]
    fn version_preference_returns_203_with_preference_applied() {
        use crate::header::NgsiLdVersion;
        use axum::response::IntoResponse;

        let prefer = PreferHeader {
            geo_json_body: GeoJsonBodyPreference::default(),
            requested_version: Some(NgsiLdVersion::V1_3),
        };
        let response = NgsiLdResponse::ok(serde_json::json!({"id": "urn:a", "type": "T"}))
            .with_prefer(prefer)
            .into_response();

        assert_eq!(response.status(), StatusCode::NON_AUTHORITATIVE_INFORMATION);
        let pref_applied = response.headers().get("Preference-Applied").and_then(|v| v.to_str().ok());
        assert_eq!(pref_applied, Some("ngsi-ld=1.9"));
    }

    #[test]
    fn no_prefer_returns_200_without_preference_applied() {
        use axum::response::IntoResponse;
        let response = NgsiLdResponse::ok(serde_json::json!({"id": "urn:a", "type": "T"})).into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().get("Preference-Applied").is_none());
    }
}
