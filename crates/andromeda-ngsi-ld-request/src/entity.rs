//! Query parameter types for entity API endpoints.

use crate::case_insensitive_bool::deserialize_option_bool_case_insensitive;
use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
use serde::Deserialize;

/// Query parameters for `POST /entities/{id}/attrs` (append attributes).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppendParams {
    /// The legacy representation selector, comma-separated (`options`).
    pub options: Option<String>,
}

/// Query parameters for `PATCH /entities/{id}/attrs/{attrId}` (partial update).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartialUpdateAttrParams {
    /// Dataset instances to select, comma-separated (`datasetId`).
    #[serde(rename = "datasetId")]
    pub dataset_id: Option<String>,
}

/// Query parameters for `PUT /entities/{id}/attrs/{attrId}` (replace attribute).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplaceAttrParams {
    /// Dataset instances to select, comma-separated (`datasetId`).
    #[serde(rename = "datasetId")]
    pub dataset_id: Option<String>,
}

/// Query parameters for `GET /entities/{id}` (retrieve single entity).
///
/// Supports format selection, attribute filtering, join traversal,
/// dataset selection, language negotiation, and snapshot/entity-map scoping.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
pub struct GetEntityParams {
    /// Attributes the response is narrowed to, comma-separated (`attrs`).
    pub attrs: Option<String>,
    /// The legacy representation selector, comma-separated (`options`).
    pub options: Option<String>,
    /// The representation the response uses (clause 4.5.1 `format`).
    pub format: Option<String>,
    /// Members the response is narrowed to (clause 6.3.15 `pick`).
    pub pick: Option<String>,
    /// Members the response drops (clause 6.3.15 `omit`).
    pub omit: Option<String>,
    /// Retained for `deny_unknown_fields` acceptance; actual parsing handled by
    /// the language-negotiation extractor.
    pub lang: Option<String>,
    /// Dataset instances to select, comma-separated (`datasetId`).
    #[serde(rename = "datasetId")]
    pub dataset_id: Option<String>,
    /// The GeoProperty a GeoJSON response uses as its geometry (clause 6.3.8 `geometryProperty`).
    #[serde(rename = "geometryProperty")]
    pub geometry_property: Option<String>,
    /// Whether the operation is confined to this broker (clause 5.10 `local`).
    #[serde(default, deserialize_with = "deserialize_option_bool_case_insensitive")]
    pub local: Option<bool>,
    /// How linked entities reach the response (clause 4.5.21 `join`).
    pub join: Option<String>,
    /// How many links deep the traversal goes (`joinLevel`).
    #[serde(rename = "joinLevel")]
    pub join_level: Option<u32>,
    /// Entity identifiers the result must be linked from (`containedBy`).
    #[serde(rename = "containedBy")]
    pub contained_by: Option<String>,
    /// Whether the query creates an entity map for stable pagination (`entityMap`).
    #[serde(rename = "entityMap")]
    pub entity_map: Option<String>,
}

impl GetEntityParams {
    /// Determine the [`ResponseFormat`] from the `format` and `options` query parameters.
    ///
    /// `format` parameter takes precedence per NGSI-LD spec.
    pub fn resolve_render_mode(&self) -> ResponseFormat {
        let fmt = self.format.as_deref().unwrap_or("");
        let opts = self.options.as_deref().unwrap_or("");
        if fmt == "keyValues" || fmt == "simplified" || opts.contains("keyValues") || opts.contains("simplified") {
            ResponseFormat::Simplified
        } else if fmt == "concise" || opts.contains("concise") {
            ResponseFormat::Concise
        } else {
            ResponseFormat::Normalized
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_params() -> GetEntityParams {
        GetEntityParams {
            attrs: None,
            options: None,
            format: None,
            pick: None,
            omit: None,
            lang: None,
            dataset_id: None,
            geometry_property: None,
            local: None,
            join: None,
            join_level: None,
            contained_by: None,
            entity_map: None,
        }
    }

    #[test]
    fn resolve_render_mode_default_is_normalized() {
        let params = default_params();
        pretty_assertions::assert_eq!(params.resolve_render_mode(), ResponseFormat::Normalized);
    }

    #[test]
    fn resolve_render_mode_format_key_values() {
        let mut params = default_params();
        params.format = Some("keyValues".to_string());
        pretty_assertions::assert_eq!(params.resolve_render_mode(), ResponseFormat::Simplified);
    }

    #[test]
    fn resolve_render_mode_format_simplified() {
        let mut params = default_params();
        params.format = Some("simplified".to_string());
        pretty_assertions::assert_eq!(params.resolve_render_mode(), ResponseFormat::Simplified);
    }

    #[test]
    fn resolve_render_mode_options_concise() {
        let mut params = default_params();
        params.options = Some("concise".to_string());
        pretty_assertions::assert_eq!(params.resolve_render_mode(), ResponseFormat::Concise);
    }

    #[test]
    fn resolve_render_mode_format_concise() {
        let mut params = default_params();
        params.format = Some("concise".to_string());
        pretty_assertions::assert_eq!(params.resolve_render_mode(), ResponseFormat::Concise);
    }

    #[test]
    fn resolve_render_mode_options_key_values() {
        let mut params = default_params();
        params.options = Some("keyValues".to_string());
        pretty_assertions::assert_eq!(params.resolve_render_mode(), ResponseFormat::Simplified);
    }

    #[test]
    fn resolve_render_mode_format_takes_precedence_over_options() {
        let mut params = default_params();
        params.format = Some("keyValues".to_string());
        params.options = Some("concise".to_string());
        pretty_assertions::assert_eq!(params.resolve_render_mode(), ResponseFormat::Simplified);
    }
}
