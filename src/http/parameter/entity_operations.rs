//! Query parameter types for batch entity operation endpoints.

use crate::http::parameter::deserialize_option_bool_case_insensitive;
use serde::Deserialize;

/// Query parameters for `POST /entityOperations/upsert`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpsertParams {
    pub options: Option<String>,
}

/// Query parameters for `PATCH /entityOperations/update`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateParams {
    pub options: Option<String>,
}

/// Query parameters for `POST /entityOperations/query`.
///
/// Supports format selection, language negotiation, join traversal,
/// dataset selection, and local-only mode.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostQueryParams {
    pub options: Option<String>,
    pub format: Option<String>,
    /// Retained for `deny_unknown_fields` acceptance; actual parsing handled by
    /// the [`LanguageNegotiation`] extractor.
    #[allow(dead_code)]
    pub lang: Option<String>,
    pub join: Option<String>,
    #[serde(rename = "joinLevel")]
    pub join_level: Option<u32>,
    #[serde(rename = "geometryProperty")]
    pub geometry_property: Option<String>,
    #[serde(rename = "datasetId")]
    pub dataset_id: Option<String>,
    pub pick: Option<String>,
    pub omit: Option<String>,
    #[serde(default, deserialize_with = "deserialize_option_bool_case_insensitive")]
    pub local: Option<bool>,
}
