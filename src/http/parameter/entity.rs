//! Query parameter types for entity API endpoints.

use crate::http::parameter::deserialize_option_bool_case_insensitive;
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
