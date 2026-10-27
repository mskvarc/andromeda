//! Conversion of the canonical clause-5.2.23 body into temporal-query parameters.

use crate::{
    attribute_selection::{AttrsAdmission, validate_attrs},
    error::ParameterError,
    pagination::parameters::PageParameters,
    query::{deserialization::OptionalGeoQuery, entity_selector::EntitySelectorParams, temporal::TemporalQueryParameters},
};
use andromeda_http_headers::language::LanguageFilter;
use andromeda_ngsi_ld_model::query::body::Query;
use andromeda_ngsi_ld_query::projection::exclusivity::{Presence, ProjectionSelectors, check_projection_exclusivity};

impl TryFrom<Query> for TemporalQueryParameters {
    type Error = ParameterError;

    fn try_from(query: Query) -> Result<TemporalQueryParameters, ParameterError> {
        let temporal = query.temporal_q.ok_or(ParameterError::TemporalQueryMissing)?;
        let aggregation = match query.aggr_params {
            Some(_) if temporal.aggr_methods.is_some() || temporal.aggr_period_duration.is_some() => {
                return Err(ParameterError::TemporalAggregationConflict);
            }
            // `aggrGroupBy` is an aggregation member, so it travels with the rest
            // of `aggrParams`; a standard `temporalQ` carries none.
            Some(aggregation) => (Some(aggregation.aggr_methods), aggregation.aggr_period_duration, aggregation.aggr_group_by),
            None => (temporal.aggr_methods, temporal.aggr_period_duration, None),
        };
        let entity_selectors: Vec<EntitySelectorParams> = query
            .entities
            .unwrap_or_default()
            .into_iter()
            .map(|selector| EntitySelectorParams {
                id: selector.id,
                id_pattern: selector.id_pattern,
                type_: Some(selector.type_),
            })
            .collect();
        let (id, entity_type, id_pattern) = match entity_selectors.as_slice() {
            [selector] => (selector.id.clone(), selector.type_.clone(), selector.id_pattern.clone()),
            [] | [_, _, ..] => (None, None, None),
        };

        let parameters = TemporalQueryParameters {
            id,
            entity_type,
            id_pattern,
            entity_selectors,
            attrs: query.attrs,
            q: query.q,
            geo: OptionalGeoQuery(query.geo_q),
            timerel: Some(temporal.timerel),
            time_at: Some(temporal.time_at),
            end_time_at: temporal.end_time_at,
            timeproperty: temporal.timeproperty,
            last_n: temporal.last_n,
            format: None,
            options: None,
            aggr_methods: aggregation.0,
            aggr_period_duration: aggregation.1,
            aggr_period_origin: None,
            aggr_group_by: aggregation.2,
            // Table 5.2.23-1 defines no pagination member: the POST binding
            // reads its page from the URL (clause 6.24.3.1).
            page: PageParameters::default(),
            count: None,
            ordering: query.ordering.unwrap_or_default(),
            pick: query.pick,
            omit: query.omit,
            join: query.join,
            join_level: query.join_level,
            contained_by: query.contained_by,
            dataset_id: query.dataset_id,
            lang: query.lang.map(LanguageFilter::from_tag),
            local: None,
            entity_map: query.entity_map,
            entity_map_lifetime: query.entity_map_lifetime,
            split_entities: query.split_entities,
            scope_q: query.scope_q,
            csf: query.csf,
            expand_values: query.expand_values,
            json_keys: query.json_keys,
        };
        // Table 5.2.23-1 restricts the `Query` datatype's `attrs` member to
        // "Attribute name as short hand strings or URIs", the same restriction
        // Table 6.18.3.2-1 puts on the query string. A body carrying a
        // `temporalQ` is answered in the temporal representation, which clause
        // 4.18 reifies the Scope in, so `scope` is an Attribute name here.
        validate_attrs(parameters.attrs.as_deref(), AttrsAdmission::ReifiedScopeAdmitted)?;
        // Clause 4.21 governs the POST body's projection exactly as it
        // governs the query string's, so the body cannot smuggle two selectors
        // past the rule the GET binding enforces.
        check_projection_exclusivity(ProjectionSelectors {
            pick: Presence::of(parameters.pick.as_deref()),
            omit: Presence::of(parameters.omit.as_deref()),
            attrs: Presence::of(parameters.attrs.as_deref()),
        })?;
        Ok(parameters)
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::query::temporal::TemporalQueryParameters;
    use andromeda_ngsi_ld_lexicon::{aggr_method::AggrMethod, identifier::Identifier, time_prop::TimeProp, time_rel::TimeRel};
    use andromeda_ngsi_ld_model::query::body::Query;
    use andromeda_ngsi_ld_representation::join_mode::JoinMode;
    use std::num::NonZeroU32;

    fn canonical(body: &str) -> Query {
        serde_json::from_str::<Query>(body).expect("valid canonical temporal query")
    }

    /// The POST body carries both ends of the projection as the query string
    /// does (clause 5.2.23 `Query`).
    #[test]
    fn a_canonical_temporal_query_naming_pick_and_omit_keeps_both() {
        let request = canonical(r#"{"type":"Query","pick":["speed"],"omit":["fuel"],"temporalQ":{"timerel":"after","timeAt":"2024-01-01T00:00:00Z"}}"#);
        let params = TemporalQueryParameters::try_from(request).expect("pick and omit may be combined");
        assert!(params.pick.is_some() && params.omit.is_some());
    }

    /// The POST body carries the join the same way the query string does
    /// (clause 5.2.23 `Query`).
    #[test]
    fn a_canonical_temporal_query_preserves_the_join_members() {
        let request = canonical(
            r#"{"type":"Query","join":"inline","joinLevel":2,"containedBy":["urn:ngsi-ld:T:1"],"temporalQ":{"timerel":"after","timeAt":"2024-01-01T00:00:00Z"}}"#,
        );
        let params = TemporalQueryParameters::try_from(request).expect("applicable temporal query");
        pretty_assertions::assert_eq!(params.join, Some(JoinMode::Inline));
        pretty_assertions::assert_eq!(params.join_level.map(NonZeroU32::get), Some(2));
        pretty_assertions::assert_eq!(
            params.contained_by.as_deref().map(|ids| ids.iter().map(Identifier::as_str).collect::<Vec<_>>()),
            Some(vec!["urn:ngsi-ld:T:1"])
        );
    }

    #[test]
    fn one_temporal_body_selector_populates_the_federation_summary() {
        let request = canonical(
            r#"{"type":"Query","entities":[{"type":"Vehicle","id":"urn:ngsi-ld:Vehicle:1"}],"temporalQ":{"timerel":"after","timeAt":"2024-01-01T00:00:00Z"}}"#,
        );
        let params = TemporalQueryParameters::try_from(request).expect("applicable temporal query");
        pretty_assertions::assert_eq!(params.id.as_deref().map(|ids| ids[0].as_str()), Some("urn:ngsi-ld:Vehicle:1"));
        pretty_assertions::assert_eq!(params.entity_type.as_ref().map(ToString::to_string).as_deref(), Some("Vehicle"));
    }

    #[test]
    fn canonical_temporal_query_requires_temporal_q() {
        assert!(TemporalQueryParameters::try_from(canonical(r#"{"type":"Query"}"#)).is_err());
    }

    #[test]
    fn canonical_temporal_query_preserves_temporal_members() {
        let request = canonical(
            r#"{"type":"Query","attrs":["speed"],"temporalQ":{"timerel":"after","timeAt":"2024-01-01T00:00:00Z","timeproperty":"observedAt","aggrMethods":"avg","aggrPeriodDuration":"PT1H","lastN":10}}"#,
        );
        let params = TemporalQueryParameters::try_from(request).expect("applicable temporal query");
        pretty_assertions::assert_eq!(params.timerel, Some(TimeRel::After));
        pretty_assertions::assert_eq!(params.time_at, Some("2024-01-01T00:00:00Z".parse().expect("valid RFC 3339 instant")));
        pretty_assertions::assert_eq!(params.timeproperty, Some(TimeProp::Observed));
        pretty_assertions::assert_eq!(params.aggr_methods, Some(vec![AggrMethod::Avg]));
        pretty_assertions::assert_eq!(params.aggr_period_duration.map(|period| period.as_iso8601()), Some("PT1H".to_string()));
        pretty_assertions::assert_eq!(params.attrs.as_deref().map(|names| names[0].as_str()), Some("speed"));
        pretty_assertions::assert_eq!(params.last_n.map(NonZeroU32::get), Some(10));
    }

    /// Clause 5.7.4.3: the context source filter is an input of the temporal
    /// query, and the canonical body carries it under the same `csf` member
    /// the entity query reads.
    #[test]
    fn canonical_temporal_query_preserves_the_context_source_filter() {
        let request = canonical(r#"{"type":"Query","csf":"tier==\"gold\"","temporalQ":{"timerel":"after","timeAt":"2024-01-01T00:00:00Z"}}"#);
        let params = TemporalQueryParameters::try_from(request).expect("applicable temporal query");
        pretty_assertions::assert_eq!(params.csf.as_ref().map(ToString::to_string).as_deref(), Some("tier==\"gold\""));
    }
}
