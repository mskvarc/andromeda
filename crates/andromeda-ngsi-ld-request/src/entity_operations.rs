//! Query parameter types for batch entity operation endpoints.

use crate::{
    case_insensitive_bool::deserialize_option_bool_case_insensitive,
    language_filter::deserialize_optional_language_filter,
    options::{EntityRepresentationOptions, OverwritePolicy, UpsertMode, deserialize_entity_representation_options, deserialize_overwrite_policy},
    query::deserialization::{deserialize_optional_dataset_selectors, deserialize_optional_projection_terms},
    representation_params::deserialize_option_response_format,
};
use andromeda_http_headers::language::LanguageFilter;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::{dataset::DatasetSelector, projection::grammar::RawProjectionTerm};
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
/// Table 5.2.23-1 gives the `Query` data type no pagination member, no
/// representation member and no local-scope flag, while Table 6.2-1 lists
/// `POST /entityOperations/query` among the HTTP operations of clause 5.7.2 —
/// one of the operations clause 4.12 lists. Clauses 6.3.10 and 6.3.13
/// therefore require `limit`, `offset` and `count` on this binding as much as
/// on the GET one, and they can only reach it through the URL.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostQueryParams {
    /// The page size (clause 6.3.10 `limit`).
    pub limit: Option<u32>,
    /// How many matches to skip before the page (clause 6.3.10 `offset`).
    pub offset: Option<u32>,
    /// Whether the response reports the total number of matches
    /// (clause 6.3.13 `count`).
    #[serde(default, deserialize_with = "deserialize_option_bool_case_insensitive")]
    pub count: Option<bool>,
    /// Typed representation and system-attribute options (`options`).
    #[serde(default, deserialize_with = "deserialize_entity_representation_options")]
    pub options: Option<EntityRepresentationOptions>,
    /// The representation the response uses (clause 4.5.1 `format`).
    #[serde(default, deserialize_with = "deserialize_option_response_format")]
    pub format: Option<ResponseFormat>,
    /// Clause-4.15 natural-language selection preferences.
    #[serde(default, deserialize_with = "deserialize_optional_language_filter")]
    pub lang: Option<LanguageFilter>,
    /// How linked entities reach the response (clause 4.5.23 `join`).
    pub join: Option<JoinMode>,
    /// How many links deep the traversal goes (`joinLevel`).
    #[serde(rename = "joinLevel")]
    pub join_level: Option<NonZeroU32>,
    /// The `GeoProperty` a `GeoJSON` response uses as its geometry (clause 6.3.15 `geometryProperty`).
    #[serde(rename = "geometryProperty")]
    pub geometry_property: Option<AttributeName>,
    /// Dataset instances to select, comma-separated (`datasetId`).
    #[serde(rename = "datasetId")]
    #[serde(default, deserialize_with = "deserialize_optional_dataset_selectors")]
    pub dataset_id: Option<Vec<DatasetSelector>>,
    /// Members the response is narrowed to (clause 4.21 `pick`).
    #[serde(default, deserialize_with = "deserialize_optional_projection_terms")]
    pub pick: Option<Vec<RawProjectionTerm>>,
    /// Members the response drops (clause 4.21 `omit`).
    #[serde(default, deserialize_with = "deserialize_optional_projection_terms")]
    pub omit: Option<Vec<RawProjectionTerm>>,
    /// Whether the operation is confined to this broker (clause 5.10 `local`).
    #[serde(default, deserialize_with = "deserialize_option_bool_case_insensitive")]
    pub local: Option<bool>,
}
