//! Query parameter types for entity API endpoints.

use crate::{
    attribute_selection::validate_attrs,
    case_insensitive_bool::deserialize_option_bool_case_insensitive,
    error::ParameterError,
    language_filter::deserialize_optional_language_filter,
    options::{EntityRepresentationOptions, OverwritePolicy, deserialize_entity_representation_options, deserialize_overwrite_policy},
    query::deserialization::{
        deserialize_optional_attribute_names,
        deserialize_optional_dataset_selector,
        deserialize_optional_dataset_selectors,
        deserialize_optional_uri_csv,
    },
    representation_params::deserialize_option_response_format,
};
use andromeda_http_headers::language::LanguageFilter;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_query::dataset::DatasetSelector;
use andromeda_ngsi_ld_representation::{join_mode::JoinMode, response_format::ResponseFormat};
use serde::Deserialize;
use std::num::NonZeroU32;

/// Query parameters for `POST /entities/{id}/attrs` (append attributes).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppendParams {
    /// Whether existing attributes may be overwritten.
    #[serde(default, deserialize_with = "deserialize_overwrite_policy")]
    pub options: Option<OverwritePolicy>,
}

/// Query parameters for `PATCH /entities/{id}/attrs/{attrId}` (partial update).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartialUpdateAttrParams {
    /// Dataset instances to select, comma-separated (`datasetId`).
    #[serde(rename = "datasetId")]
    #[serde(default, deserialize_with = "deserialize_optional_dataset_selector")]
    pub dataset_id: Option<DatasetSelector>,
}

/// Query parameters for `PUT /entities/{id}/attrs/{attrId}` (replace attribute).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplaceAttrParams {
    /// Dataset instances to select, comma-separated (`datasetId`).
    #[serde(rename = "datasetId")]
    #[serde(default, deserialize_with = "deserialize_optional_dataset_selector")]
    pub dataset_id: Option<DatasetSelector>,
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
    /// Typed representation and system-attribute options (`options`).
    #[serde(default, deserialize_with = "deserialize_entity_representation_options")]
    pub options: Option<EntityRepresentationOptions>,
    /// The representation the response uses (clause 4.5.1 `format`).
    #[serde(default, deserialize_with = "deserialize_option_response_format")]
    pub format: Option<ResponseFormat>,
    /// Members the response is narrowed to (clause 6.3.15 `pick`).
    #[serde(default, deserialize_with = "deserialize_optional_attribute_names")]
    pub pick: Option<Vec<AttributeName>>,
    /// Members the response drops (clause 6.3.15 `omit`).
    #[serde(default, deserialize_with = "deserialize_optional_attribute_names")]
    pub omit: Option<Vec<AttributeName>>,
    /// Clause-4.15 natural-language selection preferences.
    #[serde(default, deserialize_with = "deserialize_optional_language_filter")]
    pub lang: Option<LanguageFilter>,
    /// Dataset instances to select, comma-separated (`datasetId`).
    #[serde(rename = "datasetId")]
    #[serde(default, deserialize_with = "deserialize_optional_dataset_selectors")]
    pub dataset_id: Option<Vec<DatasetSelector>>,
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
        self.format.or_else(|| self.options.and_then(|options| options.format)).unwrap_or_default()
    }

    /// Reject a `format` or `options` value outside the clause 4.5.1 grammar.
    ///
    /// A single-entity GET carries the same representation parameters as a
    /// query, so an unknown `format` (`?format=bananas`) is a client error here
    /// too and must return 400 rather than being silently ignored.
    pub fn validate(&self) -> Result<(), ParameterError> {
        validate_attrs(self.attrs.as_deref())
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
        let params: GetEntityParams = serde_urlencoded::from_str("options=concise").expect("valid options");
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
        let params: GetEntityParams = serde_urlencoded::from_str("options=keyValues").expect("valid options");
        pretty_assertions::assert_eq!(params.resolve_render_mode(), ResponseFormat::Simplified);
    }

    #[test]
    fn resolve_render_mode_format_takes_precedence_over_options() {
        // Clause 4.5.1: `format` is the current representation selector and
        // `options` its legacy predecessor, so an explicit `format` wins over a
        // conflicting `options`. The revealing case is one where the two
        // disagree: `format=normalized` with `options=keyValues` must render
        // normalized, not simplified.
        let params: GetEntityParams = serde_urlencoded::from_str("format=normalized&options=keyValues").expect("valid options");
        pretty_assertions::assert_eq!(params.resolve_render_mode(), ResponseFormat::Normalized);

        let params: GetEntityParams = serde_urlencoded::from_str("format=concise&options=keyValues").expect("valid options");
        pretty_assertions::assert_eq!(params.resolve_render_mode(), ResponseFormat::Concise);
    }

    #[test]
    fn validate_rejects_unknown_format() {
        assert!(serde_urlencoded::from_str::<GetEntityParams>("format=bananas").is_err());
    }

    #[test]
    fn validate_rejects_unknown_options_token() {
        assert!(serde_urlencoded::from_str::<GetEntityParams>("options=keyValues%2Cbogus").is_err());
    }

    #[test]
    fn validate_accepts_defined_values() {
        let params: GetEntityParams = serde_urlencoded::from_str("format=concise&options=sysAttrs").expect("valid options");
        assert!(params.validate().is_ok());
    }

    #[test]
    fn mutation_dataset_id_is_typed_at_deserialization() {
        let params: PartialUpdateAttrParams = serde_urlencoded::from_str("datasetId=urn:ngsi-ld:Dataset:test").expect("valid dataset id");
        pretty_assertions::assert_eq!(params.dataset_id.as_ref().map(ToString::to_string).as_deref(), Some("urn:ngsi-ld:Dataset:test"));
        assert!(serde_urlencoded::from_str::<ReplaceAttrParams>("datasetId=not-a-urn").is_err());
    }

    #[test]
    fn entity_map_lifetime_is_typed_at_deserialization() {
        let params: crate::query::entity::parameters::EntityQueryParameters =
            serde_urlencoded::from_str("type=Vehicle&entityMapLifetime=P1M").expect("valid duration");
        pretty_assertions::assert_eq!(params.entity_map_lifetime.as_ref().map(ToString::to_string).as_deref(), Some("P1M"));
        assert!(serde_urlencoded::from_str::<crate::query::entity::parameters::EntityQueryParameters>("type=Vehicle&entityMapLifetime=bad").is_err());
    }
}
