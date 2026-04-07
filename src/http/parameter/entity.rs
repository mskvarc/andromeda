//! Query parameter types for entity API endpoints.

use crate::{http::parameter::deserialize_option_bool_case_insensitive, renderer::entity::RenderMode};
use serde::Deserialize;

/// Query parameters for `POST /entities/{id}/attrs` (append attributes).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppendParams {
    pub options: Option<String>,
}

/// Query parameters for `PATCH /entities/{id}/attrs/{attrId}` (partial update).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartialUpdateAttrParams {
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
    pub attrs: Option<String>,
    pub options: Option<String>,
    pub format: Option<String>,
    pub pick: Option<String>,
    pub omit: Option<String>,
    /// Retained for `deny_unknown_fields` acceptance; actual parsing handled by
    /// the [`LanguageNegotiation`] extractor.
    pub lang: Option<String>,
    #[serde(rename = "datasetId")]
    pub dataset_id: Option<String>,
    #[serde(rename = "geometryProperty")]
    pub geometry_property: Option<String>,
    #[serde(default, deserialize_with = "deserialize_option_bool_case_insensitive")]
    pub local: Option<bool>,
    pub join: Option<String>,
    #[serde(rename = "joinLevel")]
    pub join_level: Option<u32>,
    #[serde(rename = "containedBy")]
    pub contained_by: Option<String>,
    #[serde(rename = "entityMap")]
    pub entity_map: Option<String>,
}

impl GetEntityParams {
    /// Determine the [`RenderMode`] from the `format` and `options` query parameters.
    ///
    /// `format` parameter takes precedence per NGSI-LD spec.
    pub fn resolve_render_mode(&self) -> RenderMode {
        let fmt = self.format.as_deref().unwrap_or("");
        let opts = self.options.as_deref().unwrap_or("");
        if fmt == "keyValues" || fmt == "simplified" || opts.contains("keyValues") || opts.contains("simplified") {
            RenderMode::Simplified
        } else if fmt == "concise" || opts.contains("concise") {
            RenderMode::Concise
        } else {
            RenderMode::Normalized
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
        assert_eq!(params.resolve_render_mode(), RenderMode::Normalized);
    }

    #[test]
    fn resolve_render_mode_format_key_values() {
        let mut params = default_params();
        params.format = Some("keyValues".to_string());
        assert_eq!(params.resolve_render_mode(), RenderMode::Simplified);
    }

    #[test]
    fn resolve_render_mode_format_simplified() {
        let mut params = default_params();
        params.format = Some("simplified".to_string());
        assert_eq!(params.resolve_render_mode(), RenderMode::Simplified);
    }

    #[test]
    fn resolve_render_mode_options_concise() {
        let mut params = default_params();
        params.options = Some("concise".to_string());
        assert_eq!(params.resolve_render_mode(), RenderMode::Concise);
    }

    #[test]
    fn resolve_render_mode_format_concise() {
        let mut params = default_params();
        params.format = Some("concise".to_string());
        assert_eq!(params.resolve_render_mode(), RenderMode::Concise);
    }

    #[test]
    fn resolve_render_mode_options_key_values() {
        let mut params = default_params();
        params.options = Some("keyValues".to_string());
        assert_eq!(params.resolve_render_mode(), RenderMode::Simplified);
    }

    #[test]
    fn resolve_render_mode_format_takes_precedence_over_options() {
        let mut params = default_params();
        params.format = Some("keyValues".to_string());
        params.options = Some("concise".to_string());
        assert_eq!(params.resolve_render_mode(), RenderMode::Simplified);
    }
}
