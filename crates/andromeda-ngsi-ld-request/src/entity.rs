//! Query parameter types for entity API endpoints.

use crate::{
    attribute_selection::validate_attrs,
    case_insensitive_bool::deserialize_option_bool_case_insensitive,
    error::ParameterError,
    language_filter::deserialize_optional_language_filter,
    language_tag::deserialize_optional_language_tag,
    linked_retrieval_consistency::validate_projection_join_depth,
    options::{EntityRepresentationOptions, OverwritePolicy, deserialize_entity_representation_options, deserialize_overwrite_policy},
    query::deserialization::{
        deserialize_optional_attribute_names,
        deserialize_optional_dataset_selector,
        deserialize_optional_dataset_selectors,
        deserialize_optional_iso8601_duration,
        deserialize_optional_projection_terms,
        deserialize_optional_uri_csv,
    },
    representation_params::deserialize_option_response_format,
};
use andromeda_http_headers::language::LanguageFilter;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_query::{dataset::DatasetSelector, projection::grammar::RawProjectionTerm, type_selection::grammar::TypeExprRaw};
use andromeda_ngsi_ld_representation::{join_mode::JoinMode, merge_representation::MergeRepresentation, response_format::ResponseFormat};
use chrono::{DateTime, Utc};
use iso8601_duration::Duration as Iso8601Duration;
use langtag::LangTagBuf;
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

/// Query parameters for `PATCH /entities/{id}` (Merge Entity, Table 6.5.3.4-1).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MergeParams {
    /// The representation the fragment is written in: `simplified` (or
    /// `keyValues`) says the fragment carries bare values whose attribute
    /// types the target entity fixes.
    #[serde(default, deserialize_with = "deserialize_option_response_format")]
    pub format: Option<ResponseFormat>,
    /// The natural language of string values merged into a pre-existing
    /// `LanguageProperty`: the `languageMap` key they update.
    #[serde(default, deserialize_with = "deserialize_optional_language_tag")]
    pub lang: Option<LangTagBuf>,
    /// The `observedAt` a merged attribute that previously carried one takes
    /// when the fragment names none.
    #[serde(rename = "observedAt")]
    pub observed_at: Option<DateTime<Utc>>,
    /// The deprecated alternative to `format` (`options=simplified`).
    #[serde(default, deserialize_with = "deserialize_entity_representation_options")]
    pub options: Option<EntityRepresentationOptions>,
    /// The clause 4.17 selector the target entity's types must satisfy.
    #[serde(rename = "type")]
    pub entity_type: Option<TypeExprRaw>,
    /// Whether the operation is confined to this broker (clause 5.5.13).
    #[serde(default, deserialize_with = "deserialize_option_bool_case_insensitive")]
    pub local: Option<bool>,
}

impl MergeParams {
    /// The representation the fragment is written in: `format` wins over the
    /// deprecated `options` (Table 6.5.3.4-1), and the default is normalized.
    ///
    /// # Errors
    ///
    /// [`ParameterError::FormatInvalid`] for a representation Table 6.5.3.4-1
    /// does not admit as a merge input (`concise`).
    pub fn representation(&self) -> Result<MergeRepresentation, ParameterError> {
        let format = self.format.or_else(|| self.options.and_then(|options| options.format)).unwrap_or_default();
        MergeRepresentation::try_from(format).map_err(|error| ParameterError::FormatInvalid(error.to_string()))
    }
}

/// Query parameters for `GET /entities/{id}` (retrieve single entity).
///
/// Supports format selection, attribute filtering, join traversal,
/// dataset selection, language negotiation, and snapshot/entity-map scoping.
///
/// The default is the parameter-less retrieval, which is what a forwarded
/// Linked Entity retrieval (clause 4.5.23) starts from before adding its
/// traversal continuation.
#[derive(Debug, Default, Deserialize)]
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
    /// Members the response is narrowed to (clause 4.21 `pick`), each term
    /// carrying whatever clause 4.21 sub-projection it was written with.
    #[serde(default, deserialize_with = "deserialize_optional_projection_terms")]
    pub pick: Option<Vec<RawProjectionTerm>>,
    /// Members the response drops (clause 4.21 `omit`), each term carrying
    /// whatever clause 4.21 sub-projection it was written with.
    #[serde(default, deserialize_with = "deserialize_optional_projection_terms")]
    pub omit: Option<Vec<RawProjectionTerm>>,
    /// Clause-4.15 natural-language selection preferences.
    #[serde(default, deserialize_with = "deserialize_optional_language_filter")]
    pub lang: Option<LanguageFilter>,
    /// Dataset instances to select, comma-separated (`datasetId`).
    #[serde(rename = "datasetId")]
    #[serde(default, deserialize_with = "deserialize_optional_dataset_selectors")]
    pub dataset_id: Option<Vec<DatasetSelector>>,
    /// The `GeoProperty` a `GeoJSON` response uses as its geometry (clause 6.3.15 `geometryProperty`).
    #[serde(rename = "geometryProperty")]
    pub geometry_property: Option<AttributeName>,
    /// Whether the operation is confined to this broker (clause 5.10 `local`).
    #[serde(default, deserialize_with = "deserialize_option_bool_case_insensitive")]
    pub local: Option<bool>,
    /// How linked entities reach the response (clause 4.5.23 `join`).
    pub join: Option<JoinMode>,
    /// How many links deep the traversal goes (`joinLevel`).
    #[serde(rename = "joinLevel")]
    pub join_level: Option<NonZeroU32>,
    /// Entity identifiers the result must be linked from (`containedBy`).
    #[serde(rename = "containedBy")]
    #[serde(default, deserialize_with = "deserialize_optional_uri_csv")]
    pub contained_by: Option<Vec<Uri>>,
    /// Whether the retrieval returns the location of the `EntityMap` it used,
    /// creating one when none is in use (Table 6.5.3.1-1 `entityMap`).
    #[serde(rename = "entityMap")]
    #[serde(default, deserialize_with = "deserialize_option_bool_case_insensitive")]
    pub entity_map: Option<bool>,
    /// How long a created entity map lives, as an ISO 8601 duration
    /// (Table 6.5.3.1-1 `entityMapLifetime`).
    #[serde(rename = "entityMapLifetime")]
    #[serde(default, deserialize_with = "deserialize_optional_iso8601_duration")]
    pub entity_map_lifetime: Option<Iso8601Duration>,
}

impl GetEntityParams {
    /// Determine the [`ResponseFormat`] from the `format` and `options` query parameters.
    ///
    /// `format` takes precedence over the legacy `options` (clause 4.5.1). The
    /// resolution is shared with the GET-query parameter set so the two paths
    /// cannot disagree; see [`resolve_response_format`].
    #[must_use]
    pub fn resolve_render_mode(&self) -> ResponseFormat {
        self.format.or_else(|| self.options.and_then(|options| options.format)).unwrap_or_default()
    }

    /// Reject a `format` or `options` value outside the clause 4.5.1 grammar.
    ///
    /// A single-entity GET carries the same representation parameters as a
    /// query, so an unknown `format` (`?format=bananas`) is a client error here
    /// too and must return 400 rather than being silently ignored.
    pub fn validate(&self) -> Result<(), ParameterError> {
        validate_attrs(self.attrs.as_deref())?;
        // Clause 4.21: pick and omit select opposite ends of the same
        // projection, so a request naming both is contradictory rather than
        // a request the broker can merge. The rule is the query's rule; the
        // retrieval carries the same two parameters and so the same rule.
        if self.pick.is_some() && self.omit.is_some() {
            return Err(ParameterError::PickOmitExclusive);
        }
        if (self.pick.is_some() || self.omit.is_some()) && self.attrs.is_some() {
            return Err(ParameterError::PickOmitWithAttrs);
        }
        // Clause 5.7.2.4: a nested sub-projection describes a Linked Entity,
        // so it is only meaningful as far as the requested join reaches.
        validate_projection_join_depth(self.pick.as_deref(), self.omit.as_deref(), self.join, self.join_level)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::entity::parameters::EntityQueryParameters;

    #[test]
    fn a_retrieve_accepts_the_entity_map_flag_and_lifetime() {
        let params: GetEntityParams = serde_urlencoded::from_str("entityMap=TRUE&entityMapLifetime=PT5M").expect("parses");
        pretty_assertions::assert_eq!(params.entity_map, Some(true));
        assert!(params.entity_map_lifetime.is_some());
        assert!(serde_urlencoded::from_str::<GetEntityParams>("entityMapLifetime=5m").is_err());
        let bare: GetEntityParams = serde_urlencoded::from_str("").expect("parses");
        assert!(bare.entity_map.is_none() && bare.entity_map_lifetime.is_none());
    }

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
            entity_map_lifetime: None,
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

    /// Clause 4.21: a single-entity GET parses `pick` through the projection
    /// grammar, so `linkedTo{category}` is one term with one nested term --
    /// not the attribute name `linkedTo{category}`.
    #[test]
    fn get_pick_parses_through_the_projection_grammar() {
        let params: GetEntityParams = serde_urlencoded::from_str("pick=linkedTo%7Bcategory%7D,id").expect("valid pick");
        let terms = params.pick.as_deref().expect("pick present");
        pretty_assertions::assert_eq!(terms.len(), 2);
        pretty_assertions::assert_eq!(terms[0].name.as_str(), "linkedTo");
        pretty_assertions::assert_eq!(terms[0].linked.iter().map(|term| term.name.as_str()).collect::<Vec<_>>(), vec!["category"]);
        pretty_assertions::assert_eq!(terms[1].name.as_str(), "id");
        assert_eq!(terms[1].linked, [] as [andromeda_ngsi_ld_query::projection::grammar::RawProjectionTerm; 0]);
    }

    /// Clause 4.21 declares `pick` and `omit` mutually exclusive on every
    /// binding that accepts them, the single-entity retrieval included.
    #[test]
    fn get_validate_rejects_pick_and_omit_together() {
        let params: GetEntityParams = serde_urlencoded::from_str("pick=speed&omit=scope").expect("valid projection terms");
        let error = params.validate().expect_err("expected mutual exclusivity error");
        assert!(matches!(error, ParameterError::PickOmitExclusive));
    }

    #[test]
    fn get_validate_rejects_pick_or_omit_with_attrs() {
        for query in ["pick=speed&attrs=fuelLevel", "omit=speed&attrs=fuelLevel"] {
            let params: GetEntityParams = serde_urlencoded::from_str(query).expect("valid selectors");
            let error = params.validate().expect_err("projection and attrs are mutually exclusive");
            assert!(matches!(error, ParameterError::PickOmitWithAttrs));
        }
    }

    /// Clause 5.7.2.4: a retrieval whose `pick` nests into a Linked Entity
    /// without asking for a join names members the response cannot carry.
    #[test]
    fn get_validate_rejects_a_nested_pick_without_a_join() {
        let params: GetEntityParams = serde_urlencoded::from_str("pick=linkedTo%7Bcategory%7D").expect("valid projection terms");
        let error = params.validate().expect_err("expected a projection/join consistency error");
        assert!(matches!(error, ParameterError::ProjectionLinkedTermsWithoutJoin), "got {error:?}");
    }

    /// Clause 5.7.2.4: the nesting may not reach past the traversal, and it
    /// becomes valid as soon as `joinLevel` covers it.
    #[test]
    fn get_validate_measures_the_nesting_against_the_join_level() {
        let too_deep: GetEntityParams = serde_urlencoded::from_str("join=flat&joinLevel=1&pick=a%7Bb%7Bc%7D%7D").expect("valid projection terms");
        let error = too_deep.validate().expect_err("expected a projection/join consistency error");
        assert!(
            matches!(error, ParameterError::ProjectionDepthExceedsJoinLevel { depth: 2, join_level: 1 }),
            "got {error:?}"
        );

        let deep_enough: GetEntityParams = serde_urlencoded::from_str("join=flat&joinLevel=2&pick=a%7Bb%7Bc%7D%7D").expect("valid projection terms");
        assert!(deep_enough.validate().is_ok());
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
    fn merge_parameters_read_every_table_6_5_3_4_1_member() {
        let params: MergeParams =
            serde_urlencoded::from_str("format=simplified&lang=de&observedAt=2024-01-01T00:00:00Z&type=Vehicle&local=true").expect("parses");
        pretty_assertions::assert_eq!(params.representation().expect("simplified is a merge input"), MergeRepresentation::Simplified);
        pretty_assertions::assert_eq!(params.lang.as_ref().map(ToString::to_string).as_deref(), Some("de"));
        pretty_assertions::assert_eq!(params.observed_at.map(|at| at.to_rfc3339()).as_deref(), Some("2024-01-01T00:00:00+00:00"));
        assert!(matches!(params.entity_type, Some(TypeExprRaw::Type(ref name)) if name.as_str() == "Vehicle"));
        pretty_assertions::assert_eq!(params.local, Some(true));
    }

    #[test]
    fn merge_representation_defaults_to_normalized_prefers_format_and_rejects_concise() {
        let bare: MergeParams = serde_urlencoded::from_str("").expect("parses");
        pretty_assertions::assert_eq!(bare.representation().expect("normalized"), MergeRepresentation::Normalized);
        let options: MergeParams = serde_urlencoded::from_str("options=keyValues").expect("parses");
        pretty_assertions::assert_eq!(options.representation().expect("simplified"), MergeRepresentation::Simplified);
        let both: MergeParams = serde_urlencoded::from_str("format=normalized&options=keyValues").expect("parses");
        pretty_assertions::assert_eq!(both.representation().expect("format wins"), MergeRepresentation::Normalized);
        let concise: MergeParams = serde_urlencoded::from_str("format=concise").expect("parses");
        assert!(matches!(concise.representation(), Err(ParameterError::FormatInvalid(_))));
        assert!(serde_urlencoded::from_str::<MergeParams>("observedAt=yesterday").is_err());
        assert!(serde_urlencoded::from_str::<MergeParams>("unknown=1").is_err());
    }

    #[test]
    fn entity_map_lifetime_is_typed_at_deserialization() {
        let params: EntityQueryParameters = serde_urlencoded::from_str("type=Vehicle&entityMapLifetime=P1M").expect("valid duration");
        pretty_assertions::assert_eq!(params.entity_map_lifetime.as_ref().map(ToString::to_string).as_deref(), Some("P1M"));
        assert!(serde_urlencoded::from_str::<EntityQueryParameters>("type=Vehicle&entityMapLifetime=bad").is_err());
    }
}
