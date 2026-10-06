//! Query parameter types for entity API endpoints.

use crate::{
    attribute_selection::{AttrsAdmission, validate_attrs},
    case_insensitive_bool::deserialize_option_bool_case_insensitive,
    error::ParameterError,
    language_filter::deserialize_optional_language_filter,
    language_tag::deserialize_optional_language_tag,
    options::{EntityRepresentationOptions, OverwritePolicy},
    query::deserialization::{
        deserialize_optional_attribute_names,
        deserialize_optional_dataset_selectors,
        deserialize_optional_iso8601_duration,
        deserialize_optional_projection_terms,
        deserialize_optional_uri_csv,
    },
    representation_parameter::{WrittenValue, resolve_representation, supported_option},
};
use andromeda_http_headers::language::LanguageFilter;
use andromeda_linked_retrieval::admission::{LinkedOperation, LinkedRequest, admit_request};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier, rfc3339};
use andromeda_ngsi_ld_query::{
    dataset::DatasetSelector,
    projection::{
        exclusivity::{Presence, ProjectionSelectors, check_projection_exclusivity},
        grammar::RawProjectionTerm,
    },
    type_selection::grammar::TypeExprRaw,
};
use andromeda_ngsi_ld_representation::{
    join_mode::JoinMode,
    merge_representation::MergeRepresentation,
    response_format::ResponseFormat,
    sys_attrs::SysAttrsInclusion,
};
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
    pub options: Option<WrittenValue<OverwritePolicy>>,
    /// The clause 4.17 selector the target entity's types must satisfy
    /// (Table 6.6.3.1-1 `type`).
    #[serde(rename = "type")]
    pub entity_type: Option<TypeExprRaw>,
}

impl AppendParams {
    /// Whether the append may overwrite an attribute the entity already has.
    ///
    /// # Errors
    ///
    /// Returns [`ParameterError::OptionsInvalid`] for a value other than
    /// `noOverwrite`, which clause 6.3.20 answers with `InvalidRequest`.
    pub fn overwrite_policy(&self) -> Result<OverwritePolicy, ParameterError> {
        Ok(supported_option(self.options.as_ref())?.unwrap_or_default())
    }
}

/// Query parameters for `PATCH /entities/{id}/attrs/{attrId}` (Partial
/// Attribute Update, Table 6.7.3.1-1) and `PUT /entities/{id}/attrs/{attrId}`
/// (Replace Attribute, Table 6.7.3.3-1).
///
/// Each table defines `type` alone. It does not define `datasetId`: the
/// instance an Attribute Fragment addresses is named by the Fragment's own
/// `datasetId` (clauses 5.6.4.4 and 5.6.19.4), so the URL parameter is refused
/// as unknown (clause 6.3.20).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttributeWriteParams {
    /// The clause 4.17 selector the target entity's types must satisfy.
    #[serde(rename = "type")]
    pub entity_type: Option<TypeExprRaw>,
}

/// Query parameters for `PATCH /entities/{id}` (Merge Entity, Table 6.5.3.4-1).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MergeParams {
    /// The representation the fragment is written in: `simplified` (or
    /// `keyValues`) says the fragment carries bare values whose attribute
    /// types the target entity fixes.
    pub format: Option<WrittenValue<ResponseFormat>>,
    /// The natural language of string values merged into a pre-existing
    /// `LanguageProperty`: the `languageMap` key they update.
    #[serde(default, deserialize_with = "deserialize_optional_language_tag")]
    pub lang: Option<LangTagBuf>,
    /// The `observedAt` a merged attribute that previously carried one takes
    /// when the fragment names none.
    #[serde(rename = "observedAt", default, deserialize_with = "rfc3339::optional_datetime::deserialize")]
    pub observed_at: Option<DateTime<Utc>>,
    /// The deprecated alternative to `format` (`options=simplified`).
    pub options: Option<WrittenValue<EntityRepresentationOptions>>,
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
    /// [`ParameterError::MergeFormatInvalid`] for a representation Table
    /// 6.5.3.4-1 does not admit as a merge input (`concise`).
    pub fn representation(&self) -> Result<MergeRepresentation, ParameterError> {
        let format = resolve_representation(self.format.as_ref(), self.options.as_ref())?;
        MergeRepresentation::try_from(format).map_err(ParameterError::MergeFormatInvalid)
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
    /// The clause 4.17 selector the target entity's types must satisfy
    /// (Table 6.5.3.1-1 `type`): clause 5.7.1.4 knows the target only as an
    /// entity "whose id (URI), and where specified type, is equivalent".
    #[serde(rename = "type")]
    pub entity_type: Option<TypeExprRaw>,
    /// Attributes the response is narrowed to, comma-separated (`attrs`).
    #[serde(default, deserialize_with = "deserialize_optional_attribute_names")]
    pub attrs: Option<Vec<AttributeName>>,
    /// Typed representation and system-attribute options (`options`).
    pub options: Option<WrittenValue<EntityRepresentationOptions>>,
    /// The representation the response uses (clause 4.5.1 `format`).
    pub format: Option<WrittenValue<ResponseFormat>>,
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
    pub contained_by: Option<Vec<Identifier>>,
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
    /// cannot disagree; see [`resolve_representation`].
    ///
    /// # Errors
    ///
    /// Returns [`ParameterError::FormatInvalid`] or
    /// [`ParameterError::OptionsInvalid`] when either parameter named a
    /// rendering Table 6.3.7-1 does not define (clause 6.3.20).
    pub fn resolve_render_mode(&self) -> Result<ResponseFormat, ParameterError> {
        resolve_representation(self.format.as_ref(), self.options.as_ref())
    }

    /// Whether the retrieval includes the system-generated temporal attributes
    /// (clause 6.3.11 `options=sysAttrs`).
    ///
    /// # Errors
    ///
    /// Returns [`ParameterError::OptionsInvalid`] when `options` carried a
    /// value Table 6.3.7-1 does not define (clause 6.3.20).
    pub fn sys_attrs(&self) -> Result<SysAttrsInclusion, ParameterError> {
        Ok(supported_option(self.options.as_ref())?.unwrap_or_default().sys_attrs)
    }

    /// Reject a `format` or `options` value outside the clause 4.5.1 grammar.
    ///
    /// A single-entity GET carries the same representation parameters as a
    /// query, so an unknown `format` (`?format=bananas`) is a client error here
    /// too and must return 400 rather than being silently ignored.
    ///
    /// # Errors
    ///
    /// Returns [`ParameterError::AttrsReservedMember`] when `attrs` names an
    /// Entity member, [`ParameterError::Projection`] when the clause 4.21
    /// selectors contradict each other, and
    /// [`ParameterError::ProjectionLinkedTermsWithoutJoin`] or
    /// [`ParameterError::ProjectionDepthExceedsJoinLevel`] when a nested
    /// projection term names a Linked Entity the requested traversal does not
    /// reach.
    pub fn validate(&self) -> Result<(), ParameterError> {
        // Clause 6.3.20: a `format` or `options` value Table 6.3.7-1 does not
        // define is refused here rather than during deserialisation, where the
        // error type would have been lost (see `crate::representation_parameter`).
        self.resolve_render_mode()?;
        // Table 6.5.3.1-1 restricts `attrs` to "an Attribute (Property or
        // Relationship) name" and says outright that "id, type, scope are not
        // allowed". Clause 4.18 agrees for this representation: an Entity
        // retrieval leaves the Scope non-reified, so it is no Attribute here.
        validate_attrs(self.attrs.as_deref(), AttrsAdmission::AttributeNamesOnly)?;
        // Clause 4.21: the retrieval carries the same three selectors a query
        // does, and so the same rule about combining them.
        check_projection_exclusivity(ProjectionSelectors {
            pick: Presence::of(self.pick.as_deref()),
            omit: Presence::of(self.omit.as_deref()),
            attrs: Presence::of(self.attrs.as_deref()),
        })?;
        // Clause 5.7.1.4: a nested sub-projection describes a Linked Entity,
        // so it is only meaningful as far as the requested join reaches.
        let linked = LinkedRequest::builder()
            .pick(self.pick.as_deref())
            .omit(self.omit.as_deref())
            .join(self.join)
            .join_level(self.join_level)
            .build();
        admit_request(LinkedOperation::RetrieveEntity, &linked)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::entity::parameters::EntityQueryParameters;
    use andromeda_ngsi_ld_query::projection::exclusivity::ProjectionConflict;

    #[test]
    fn a_retrieve_accepts_the_entity_map_flag_and_lifetime() {
        let params: GetEntityParams = serde_urlencoded::from_str("entityMap=TRUE&entityMapLifetime=PT5M").expect("parses");
        pretty_assertions::assert_eq!(params.entity_map, Some(true));
        assert!(params.entity_map_lifetime.is_some());
        assert!(serde_urlencoded::from_str::<GetEntityParams>("entityMapLifetime=5m").is_err());
        let bare: GetEntityParams = serde_urlencoded::from_str("").expect("parses");
        assert!(bare.entity_map.is_none() && bare.entity_map_lifetime.is_none());
    }

    /// Table 6.5.3.1-1: `type` is a Retrieve Entity parameter carrying a
    /// clause 4.17 selector, and a malformed selector is a client error.
    #[test]
    fn a_retrieve_accepts_a_clause_4_17_type_selector() {
        let params: GetEntityParams = serde_urlencoded::from_str("type=Vehicle%7CBuilding").expect("a valid type selector");
        pretty_assertions::assert_eq!(params.entity_type.map(|selector| selector.to_string()), Some("Vehicle,Building".to_string()));
        let bare: GetEntityParams = serde_urlencoded::from_str("").expect("parses");
        assert!(bare.entity_type.is_none());
        assert!(serde_urlencoded::from_str::<GetEntityParams>("type=Vehicle%7C").is_err());
    }

    /// Tables 6.7.3.1-1 and 6.7.3.3-1 define the `type` parameter, a clause 4.17
    /// selector: a malformed one is a client error, and without the parameter
    /// no selector applies.
    #[test]
    fn an_attribute_write_reads_a_clause_4_17_type_selector() {
        let params: AttributeWriteParams = serde_urlencoded::from_str("type=Vehicle%7CBuilding").expect("a valid type selector");
        pretty_assertions::assert_eq!(params.entity_type.map(|selector| selector.to_string()), Some("Vehicle,Building".to_string()));
        assert!(serde_urlencoded::from_str::<AttributeWriteParams>("type=Vehicle%7C").is_err());
        // Clause 4.17's grammar has no empty selector: `type=` names no type, so
        // it is refused rather than read as no selector.
        assert!(serde_urlencoded::from_str::<AttributeWriteParams>("type=").is_err());
        let bare: AttributeWriteParams = serde_urlencoded::from_str("").expect("parses");
        assert!(bare.entity_type.is_none());
    }

    /// Table 6.6.3.1-1: Append Attributes takes `type` beside `options`.
    #[test]
    fn an_append_reads_a_type_selector_beside_its_options() {
        let params: AppendParams = serde_urlencoded::from_str("type=Vehicle&options=noOverwrite").expect("both parameters parse");
        pretty_assertions::assert_eq!(params.entity_type.map(|selector| selector.to_string()), Some("Vehicle".to_string()));
        assert!(params.options.is_some());
        let bare: AppendParams = serde_urlencoded::from_str("").expect("parses");
        assert!(bare.entity_type.is_none() && bare.options.is_none());
    }

    fn default_params() -> GetEntityParams {
        GetEntityParams {
            entity_type: None,
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
        pretty_assertions::assert_eq!(params.resolve_render_mode().expect("a supported rendering"), ResponseFormat::Normalized);
    }

    #[test]
    fn resolve_render_mode_format_key_values() {
        let mut params = default_params();
        params.format = Some(WrittenValue::Supported(ResponseFormat::Simplified));
        pretty_assertions::assert_eq!(params.resolve_render_mode().expect("a supported rendering"), ResponseFormat::Simplified);
    }

    #[test]
    fn resolve_render_mode_format_simplified() {
        let mut params = default_params();
        params.format = Some(WrittenValue::Supported(ResponseFormat::Simplified));
        pretty_assertions::assert_eq!(params.resolve_render_mode().expect("a supported rendering"), ResponseFormat::Simplified);
    }

    #[test]
    fn resolve_render_mode_options_concise() {
        let params: GetEntityParams = serde_urlencoded::from_str("options=concise").expect("valid options");
        pretty_assertions::assert_eq!(params.resolve_render_mode().expect("a supported rendering"), ResponseFormat::Concise);
    }

    #[test]
    fn resolve_render_mode_format_concise() {
        let mut params = default_params();
        params.format = Some(WrittenValue::Supported(ResponseFormat::Concise));
        pretty_assertions::assert_eq!(params.resolve_render_mode().expect("a supported rendering"), ResponseFormat::Concise);
    }

    #[test]
    fn resolve_render_mode_options_key_values() {
        let params: GetEntityParams = serde_urlencoded::from_str("options=keyValues").expect("valid options");
        pretty_assertions::assert_eq!(params.resolve_render_mode().expect("a supported rendering"), ResponseFormat::Simplified);
    }

    #[test]
    fn resolve_render_mode_format_takes_precedence_over_options() {
        // Clause 4.5.1: `format` is the current representation selector and
        // `options` its legacy predecessor, so an explicit `format` wins over a
        // conflicting `options`. The revealing case is one where the two
        // disagree: `format=normalized` with `options=keyValues` must render
        // normalized, not simplified.
        let params: GetEntityParams = serde_urlencoded::from_str("format=normalized&options=keyValues").expect("valid options");
        pretty_assertions::assert_eq!(params.resolve_render_mode().expect("a supported rendering"), ResponseFormat::Normalized);

        let params: GetEntityParams = serde_urlencoded::from_str("format=concise&options=keyValues").expect("valid options");
        pretty_assertions::assert_eq!(params.resolve_render_mode().expect("a supported rendering"), ResponseFormat::Concise);
    }

    /// Clause 6.3.20: a `format` or `options` value Table 6.3.7-1 does not
    /// define survives deserialisation, so the rejection can name the error
    /// type, and is refused by `validate`.
    #[test]
    fn validate_rejects_a_representation_the_table_does_not_define() {
        for query in ["format=bananas", "options=keyValues%2Cbogus"] {
            let params: GetEntityParams = serde_urlencoded::from_str(query).expect("the parameter set decodes");
            let refused = params.validate().expect_err("the grammar refuses it");
            assert!(
                matches!(refused, ParameterError::FormatInvalid(_) | ParameterError::OptionsInvalid(_)),
                "{query}: {refused}"
            );
        }
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
        assert_eq!(terms[1].linked, [] as [RawProjectionTerm; 0]);
    }

    /// Clause 4.21 declares `pick` and `omit` mutually exclusive on every
    /// binding that accepts them, the single-entity retrieval included.
    #[test]
    fn get_validate_rejects_pick_and_omit_together() {
        let params: GetEntityParams = serde_urlencoded::from_str("pick=speed&omit=scope").expect("valid projection terms");
        let error = params.validate().expect_err("expected mutual exclusivity error");
        assert!(matches!(error, ParameterError::Projection(ProjectionConflict::PickWithOmit)));
    }

    #[test]
    fn get_validate_rejects_pick_or_omit_with_attrs() {
        for query in ["pick=speed&attrs=fuelLevel", "omit=speed&attrs=fuelLevel"] {
            let params: GetEntityParams = serde_urlencoded::from_str(query).expect("valid selectors");
            let error = params.validate().expect_err("projection and attrs are mutually exclusive");
            assert!(matches!(error, ParameterError::Projection(ProjectionConflict::ProjectionWithAttrs)));
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

    /// Tables 6.7.3.1-1 and 6.7.3.3-1 define no `datasetId` parameter.
    #[test]
    fn an_attribute_write_refuses_a_dataset_id_parameter() {
        let error = serde_urlencoded::from_str::<AttributeWriteParams>("datasetId=urn:ngsi-ld:Dataset:test").expect_err("datasetId is not defined");
        assert!(error.to_string().contains("unknown field"), "got {error}");
    }

    /// Clause 4.6.3: a comma fraction separator is accepted in a request.
    #[test]
    fn a_merge_observed_at_with_a_comma_fraction_is_accepted() {
        let params: MergeParams = serde_urlencoded::from_str("observedAt=2024-01-01T00:00:00,5Z").expect("parses");
        pretty_assertions::assert_eq!(params.observed_at.map(|at| at.to_rfc3339()).as_deref(), Some("2024-01-01T00:00:00.500+00:00"));
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
        assert!(matches!(concise.representation(), Err(ParameterError::MergeFormatInvalid(_))));
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
