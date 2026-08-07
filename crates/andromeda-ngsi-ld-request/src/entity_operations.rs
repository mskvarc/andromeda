//! Query parameter types for batch entity operation endpoints.

use crate::{
    case_insensitive_bool::deserialize_option_bool_case_insensitive,
    query::deserialization::{deserialize_optional_attribute_names, deserialize_optional_dataset_urns},
    representation_params::deserialize_option_response_format,
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_representation::{join_mode::JoinMode, response_format::ResponseFormat};
use serde::Deserialize;
use std::num::NonZeroU32;
use urn_rs::Urn;

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
    #[serde(default, deserialize_with = "deserialize_option_response_format")]
    pub format: Option<ResponseFormat>,
    /// Retained for `deny_unknown_fields` acceptance; actual parsing handled by
    /// the language-negotiation extractor.
    #[allow(dead_code)]
    pub lang: Option<String>,
    /// How linked entities reach the response (clause 4.5.21 `join`).
    pub join: Option<JoinMode>,
    /// How many links deep the traversal goes (`joinLevel`).
    #[serde(rename = "joinLevel")]
    pub join_level: Option<NonZeroU32>,
    /// The GeoProperty a GeoJSON response uses as its geometry (clause 6.3.8 `geometryProperty`).
    #[serde(rename = "geometryProperty")]
    pub geometry_property: Option<AttributeName>,
    /// Dataset instances to select, comma-separated (`datasetId`).
    #[serde(rename = "datasetId")]
    #[serde(default, deserialize_with = "deserialize_optional_dataset_urns")]
    pub dataset_id: Option<Vec<Urn>>,
    /// Members the response is narrowed to (clause 6.3.15 `pick`).
    #[serde(default, deserialize_with = "deserialize_optional_attribute_names")]
    pub pick: Option<Vec<AttributeName>>,
    /// Members the response drops (clause 6.3.15 `omit`).
    #[serde(default, deserialize_with = "deserialize_optional_attribute_names")]
    pub omit: Option<Vec<AttributeName>>,
    /// Whether the operation is confined to this broker (clause 5.10 `local`).
    #[serde(default, deserialize_with = "deserialize_option_bool_case_insensitive")]
    pub local: Option<bool>,
}
