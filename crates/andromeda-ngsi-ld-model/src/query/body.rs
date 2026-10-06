//! The canonical model-owned `Query` resource.

use crate::{
    attribute_name_csv,
    query::{aggregation_parameters::AggregationParameters, entity_selector::QueryEntitySelector, non_empty_member, temporal_query::TemporalQuery},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, constant_string, identifier::Identifier};
use andromeda_ngsi_ld_query::{
    dataset::DatasetSelector,
    filter::expression::Expr,
    geo::input::GeoQueryInput,
    ordering::parameters::OrderingParameters,
    projection::grammar::RawProjectionTerm,
    scope::query::ScopeQuery,
};
use andromeda_ngsi_ld_representation::join_mode::JoinMode;
use iso8601_duration::Duration as Iso8601Duration;
use langtag::LangTagBuf;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;
use typed_builder::TypedBuilder;

constant_string!(QueryType, "Query");

/// A query body shared by entity queries, temporal queries, and snapshots
/// (ETSI GS CIM 009 v1.9.1 clause 5.2.23).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TypedBuilder)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[builder(field_defaults(default = Default::default()))]
pub struct Query {
    /// Required fixed resource discriminator.
    #[serde(rename = "type")]
    pub type_: QueryType,
    /// Entity selector union.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "non_empty_member::deserialize_array")]
    pub entities: Option<Vec<QueryEntitySelector>>,
    /// Clause-4.9 entity filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub q: Option<Expr>,
    /// Clause-4.10 geo-spatial filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geo_q: Option<GeoQueryInput>,
    /// Clause-4.19 scope filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_q: Option<ScopeQuery>,
    /// Temporal window and temporal-instance selection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temporal_q: Option<TemporalQuery>,
    /// Attributes that matching entities must carry.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "non_empty_member::deserialize_array")]
    pub attrs: Option<Vec<AttributeName>>,
    /// Members removed from each result entity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "projection_term_list::deserialize",
        serialize_with = "projection_term_list::serialize"
    )]
    pub omit: Option<Vec<RawProjectionTerm>>,
    /// Members retained in each result entity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "projection_term_list::deserialize",
        serialize_with = "projection_term_list::serialize"
    )]
    pub pick: Option<Vec<RawProjectionTerm>>,
    /// Temporal aggregation parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aggr_params: Option<AggregationParameters>,
    /// Clause-4.9 filter applied to candidate registrations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub csf: Option<Expr>,
    /// Entity identifiers already encountered during graph traversal.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "non_empty_member::deserialize_array")]
    pub contained_by: Option<Vec<Identifier>>,
    /// Dataset instances selected for each matching attribute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dataset_id: Option<Vec<DatasetSelector>>,
    /// Attribute values expanded through JSON-LD coercion before filtering.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "attribute_name_csv::deserialize",
        serialize_with = "attribute_name_csv::serialize"
    )]
    pub expand_values: Option<Vec<AttributeName>>,
    /// Whether an `EntityMap` location is returned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_map: Option<bool>,
    /// Requested lifetime of a generated `EntityMap`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_map_lifetime: Option<Iso8601Duration>,
    /// Attribute names whose nested values are treated as uninterpreted JSON.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "attribute_name_csv::deserialize",
        serialize_with = "attribute_name_csv::serialize"
    )]
    pub json_keys: Option<Vec<AttributeName>>,
    /// Linked-entity rendering mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub join: Option<JoinMode>,
    /// Linked-entity traversal depth.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub join_level: Option<NonZeroU32>,
    /// Validated RFC 5646 language filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lang: Option<LangTagBuf>,
    /// Entity ordering definition.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "non_empty_member::deserialize_ordering")]
    pub ordering: Option<OrderingParameters>,
    /// Whether distributed entity fragments must be reassembled before filters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split_entities: Option<bool>,
}

/// Serde support for the `pick` / `omit` clause 4.21 member.
///
/// Clause 5.2.23 defines `pick` and `omit` as `String[]`, each member a
/// clause 4.21 projection-language string that may itself carry a `,`
/// separated linked-entity group. Splitting the array first and parsing each
/// member is therefore not the same as splitting on `,` across the whole
/// member: a member's grammar owns its own comma/pipe splitting, and the
/// terms every member yields are concatenated into one flat list.
///
/// The module is public because it names the wire form of `pick` and `omit`:
/// the pair of functions is the encoding those members have, and a caller
/// rendering one of them outside a `Query` needs the same encoding.
pub mod projection_term_list {
    use crate::query::non_empty_member;
    use andromeda_ngsi_ld_query::projection::grammar::{ProjectionList, RawProjectionTerm};
    use serde::{Deserializer, Serializer, de::Error as DeserializeError};

    /// Deserialize the array of projection-language strings into the terms
    /// they parse to, concatenated in array order.
    ///
    /// # Errors
    ///
    /// Returns the deserializer's own error when the member is neither absent
    /// nor an array of strings, and a custom error when one of those strings
    /// does not parse as a clause 4.21 projection list.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Vec<RawProjectionTerm>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        non_empty_member::deserialize_array::<D, String>(deserializer)?
            .map(|members| {
                members
                    .iter()
                    .map(|member| member.parse::<ProjectionList>().map(Vec::from).map_err(DeserializeError::custom))
                    .collect::<Result<Vec<Vec<RawProjectionTerm>>, D::Error>>()
                    .map(|groups| groups.into_iter().flatten().collect())
            })
            .transpose()
    }

    /// Serialize each term as its own clause 4.21 array member.
    ///
    /// # Errors
    ///
    /// Returns the serializer's own error when the member cannot be written.
    pub fn serialize<S>(value: &Option<Vec<RawProjectionTerm>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(terms) => serializer.collect_seq(terms.iter().map(ToString::to_string)),
            None => serializer.serialize_none(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::query::body::Query;
    use andromeda_ngsi_ld_query::dataset::DatasetSelector;

    #[test]
    fn the_query_discriminator_is_required_and_exact() {
        assert!(serde_json::from_value::<Query>(serde_json::json!({"type": "Query"})).is_ok());
        assert!(serde_json::from_value::<Query>(serde_json::json!({})).is_err());
        assert!(serde_json::from_value::<Query>(serde_json::json!({"type": "Subscription"})).is_err());
    }

    #[test]
    fn unknown_and_misspelled_members_are_rejected() {
        assert!(serde_json::from_value::<Query>(serde_json::json!({"type": "Query", "unknown": true})).is_err());
        assert!(serde_json::from_value::<Query>(serde_json::json!({"type": "Query", "datasetID": []})).is_err());
    }

    #[test]
    fn dataset_selectors_accept_urn_url_and_default_instance() {
        let query: Query = serde_json::from_value(serde_json::json!({
            "type": "Query",
            "datasetId": ["urn:ngsi-ld:Dataset:one", "https://example.com/datasets/two", "@none"]
        }))
        .expect("valid datasets");
        let selectors = query.dataset_id.expect("dataset selectors");
        assert!(matches!(selectors[0], DatasetSelector::Id(_)));
        assert!(matches!(selectors[1], DatasetSelector::Id(_)));
        pretty_assertions::assert_eq!(selectors[2], DatasetSelector::Default);
    }

    #[test]
    fn invalid_language_tags_and_unknown_geometry_types_are_rejected() {
        assert!(serde_json::from_value::<Query>(serde_json::json!({"type": "Query", "lang": "not a tag"})).is_err());
        assert!(
            serde_json::from_value::<Query>(serde_json::json!({
                "type": "Query",
                "ordering": {"orderBy": ["location;dist-asc"], "geometry": "GeometryCollection", "coordinates": []}
            }))
            .is_err()
        );
    }
}
