//! Query parameter types for batch entity operation endpoints.

use crate::case_insensitive_bool::deserialize_option_bool_case_insensitive;
use serde::Deserialize;

/// Query parameters for `POST /entityOperations/upsert`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpsertParams {
    /// The legacy representation selector, comma-separated (`options`).
    pub options: Option<String>,
}

/// Query parameters for `PATCH /entityOperations/update`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateParams {
    /// The legacy representation selector, comma-separated (`options`).
    pub options: Option<String>,
}

/// Query parameters for `POST /entityOperations/query`.
///
/// Supports format selection, language negotiation, join traversal,
/// dataset selection, and local-only mode.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostQueryParams {
    /// The legacy representation selector, comma-separated (`options`).
    pub options: Option<String>,
    /// The representation the response uses (clause 4.5.1 `format`).
    pub format: Option<String>,
    /// Retained for `deny_unknown_fields` acceptance; actual parsing handled by
    /// the language-negotiation extractor.
    #[allow(dead_code)]
    pub lang: Option<String>,
    /// How linked entities reach the response (clause 4.5.21 `join`).
    pub join: Option<String>,
    /// How many links deep the traversal goes (`joinLevel`).
    #[serde(rename = "joinLevel")]
    pub join_level: Option<u32>,
    /// The GeoProperty a GeoJSON response uses as its geometry (clause 6.3.8 `geometryProperty`).
    #[serde(rename = "geometryProperty")]
    pub geometry_property: Option<String>,
    /// Dataset instances to select, comma-separated (`datasetId`).
    #[serde(rename = "datasetId")]
    pub dataset_id: Option<String>,
    /// Members the response is narrowed to (clause 6.3.15 `pick`).
    pub pick: Option<String>,
    /// Members the response drops (clause 6.3.15 `omit`).
    pub omit: Option<String>,
    /// Whether the operation is confined to this broker (clause 5.10 `local`).
    #[serde(default, deserialize_with = "deserialize_option_bool_case_insensitive")]
    pub local: Option<bool>,
}
