//! Query parameter types for entity API endpoints.

use crate::{
    case_insensitive_bool::deserialize_option_bool_case_insensitive,
    error::ParameterError,
    query::deserialization::{deserialize_optional_attribute_names, deserialize_optional_dataset_urns, deserialize_optional_uri_csv},
    representation_params::{deserialize_option_response_format, resolve_response_format, validate_representation_params},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_representation::{join_mode::JoinMode, response_format::ResponseFormat};
use serde::Deserialize;
use std::num::NonZeroU32;
use urn_rs::Urn;

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
pub struct GetEntityParams {
    /// Attributes the response is narrowed to, comma-separated (`attrs`).
    #[serde(default, deserialize_with = "deserialize_optional_attribute_names")]
    pub attrs: Option<Vec<AttributeName>>,
    /// The legacy representation selector, comma-separated (`options`).
    pub options: Option<String>,
    /// The representation the response uses (clause 4.5.1 `format`).
    #[serde(default, deserialize_with = "deserialize_option_response_format")]
    pub format: Option<ResponseFormat>,
    /// Members the response is narrowed to (clause 6.3.15 `pick`).
    #[serde(default, deserialize_with = "deserialize_optional_attribute_names")]
    pub pick: Option<Vec<AttributeName>>,
    /// Members the response drops (clause 6.3.15 `omit`).
    #[serde(default, deserialize_with = "deserialize_optional_attribute_names")]
    pub omit: Option<Vec<AttributeName>>,
    /// Retained for `deny_unknown_fields` acceptance; actual parsing handled by
    /// the language-negotiation extractor.
    pub lang: Option<String>,
    /// Dataset instances to select, comma-separated (`datasetId`).
    #[serde(rename = "datasetId")]
    #[serde(default, deserialize_with = "deserialize_optional_dataset_urns")]
    pub dataset_id: Option<Vec<Urn>>,
    /// The GeoProperty a GeoJSON response uses as its geometry (clause 6.3.8 `geometryProperty`).
    #[serde(rename = "geometryProperty")]
    pub geometry_property: Option<AttributeName>,
    /// Whether the operation is confined to this broker (clause 5.10 `local`).
    #[serde(default, deserialize_with = "deserialize_option_bool_case_insensitive")]
    pub local: Option<bool>,
    /// How linked entities reach the response (clause 4.5.21 `join`).
    pub join: Option<JoinMode>,
    /// How many links deep the traversal goes (`joinLevel`).
    #[serde(rename = "joinLevel")]
    pub join_level: Option<NonZeroU32>,
    /// Entity identifiers the result must be linked from (`containedBy`).
    #[serde(rename = "containedBy")]
    #[serde(default, deserialize_with = "deserialize_optional_uri_csv")]
    pub contained_by: Option<Vec<Uri>>,
}

impl GetEntityParams {
    /// Determine the [`ResponseFormat`] from the `format` and `options` query parameters.
    ///
    /// `format` takes precedence over the legacy `options` (clause 4.5.1). The
    /// resolution is shared with the GET-query parameter set so the two paths
    /// cannot disagree; see [`resolve_response_format`].
    pub fn resolve_render_mode(&self) -> ResponseFormat {
        resolve_response_format(self.format, self.options.as_deref())
    }

    /// Reject a `format` or `options` value outside the clause 4.5.1 grammar.
    ///
    /// A single-entity GET carries the same representation parameters as a
    /// query, so an unknown `format` (`?format=bananas`) is a client error here
    /// too and must return 400 rather than being silently ignored.
    pub fn validate(&self) -> Result<(), ParameterError> {
        validate_representation_params(self.format, self.options.as_deref())
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
        params.format = Some(ResponseFormat::Simplified);
        pretty_assertions::assert_eq!(params.resolve_render_mode(), ResponseFormat::Simplified);
    }

    #[test]
    fn resolve_render_mode_format_simplified() {
        let mut params = default_params();
        params.format = Some(ResponseFormat::Simplified);
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
        params.format = Some(ResponseFormat::Concise);
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
        // Clause 4.5.1: `format` is the current representation selector and
        // `options` its legacy predecessor, so an explicit `format` wins over a
        // conflicting `options`. The revealing case is one where the two
        // disagree: `format=normalized` with `options=keyValues` must render
        // normalized, not simplified.
        let mut params = default_params();
        params.format = Some(ResponseFormat::Normalized);
        params.options = Some("keyValues".to_string());
        pretty_assertions::assert_eq!(params.resolve_render_mode(), ResponseFormat::Normalized);

        let mut params = default_params();
        params.format = Some(ResponseFormat::Concise);
        params.options = Some("keyValues".to_string());
        pretty_assertions::assert_eq!(params.resolve_render_mode(), ResponseFormat::Concise);
    }

    #[test]
    fn validate_rejects_unknown_format() {
        assert!(serde_urlencoded::from_str::<GetEntityParams>("format=bananas").is_err());
    }

    #[test]
    fn validate_rejects_unknown_options_token() {
        let mut params = default_params();
        params.options = Some("keyValues,bogus".to_string());
        assert!(matches!(params.validate(), Err(ParameterError::OptionsInvalid(_))));
    }

    #[test]
    fn validate_accepts_defined_values() {
        let mut params = default_params();
        params.format = Some(ResponseFormat::Concise);
        params.options = Some("sysAttrs".to_string());
        assert!(params.validate().is_ok());
    }
}
