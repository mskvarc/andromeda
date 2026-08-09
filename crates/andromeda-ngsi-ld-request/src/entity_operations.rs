//! Query parameter types for batch entity operation endpoints.

use crate::{
    case_insensitive_bool::deserialize_option_bool_case_insensitive,
    language_filter::deserialize_optional_language_filter,
    options::{EntityRepresentationOptions, OverwritePolicy, UpsertMode, deserialize_entity_representation_options, deserialize_overwrite_policy},
    query::deserialization::{deserialize_optional_attribute_names, deserialize_optional_dataset_selectors},
    representation_params::deserialize_option_response_format,
};
use andromeda_http_headers::language::LanguageFilter;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::dataset::DatasetSelector;
use andromeda_ngsi_ld_representation::{join_mode::JoinMode, response_format::ResponseFormat};
use serde::Deserialize;
use std::num::NonZeroU32;

/// Query parameters for `POST /entityOperations/upsert`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpsertParams {
    /// Whether existing entities are replaced or updated.
    pub options: Option<UpsertMode>,
}

/// Query parameters for `PATCH /entityOperations/update`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateParams {
    /// Whether existing attributes may be overwritten.
    #[serde(default, deserialize_with = "deserialize_overwrite_policy")]
    pub options: Option<OverwritePolicy>,
}

/// Query parameters for `POST /entityOperations/query`.
///
/// Supports format selection, language negotiation, join traversal,
/// dataset selection, and local-only mode.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostQueryParams {
    /// Typed representation and system-attribute options (`options`).
    #[serde(default, deserialize_with = "deserialize_entity_representation_options")]
    pub options: Option<EntityRepresentationOptions>,
    /// The representation the response uses (clause 4.5.1 `format`).
    #[serde(default, deserialize_with = "deserialize_option_response_format")]
    pub format: Option<ResponseFormat>,
    /// Clause-4.15 natural-language selection preferences.
    #[serde(default, deserialize_with = "deserialize_optional_language_filter")]
    pub lang: Option<LanguageFilter>,
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
    #[serde(default, deserialize_with = "deserialize_optional_dataset_selectors")]
    pub dataset_id: Option<Vec<DatasetSelector>>,
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
