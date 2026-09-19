//! Conversion of the canonical clause-5.2.23 body into entity-query parameters.

use crate::{
    attribute_selection::{AttrsAdmission, validate_attrs},
    error::ParameterError,
    query::{deserialization::OptionalGeoQuery, entity::parameters::EntityQueryParameters, entity_selector::EntitySelectorParams},
};
use andromeda_http_headers::language::LanguageFilter;
use andromeda_ngsi_ld_model::query::body::Query;

impl TryFrom<Query> for EntityQueryParameters {
    type Error = ParameterError;

    fn try_from(query: Query) -> Result<EntityQueryParameters, ParameterError> {
        if query.temporal_q.is_some() || query.aggr_params.is_some() {
            return Err(ParameterError::EntityQueryTemporalMembers);
        }

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

        let parameters = EntityQueryParameters {
            id,
            entity_type,
            id_pattern,
            entity_selectors,
            attrs: query.attrs,
            q: query.q,
            geo: OptionalGeoQuery(query.geo_q),
            limit: None,
            offset: None,
            // The POST query binding carries no dereferenceable pointer
            // (clause 6.23.3.1), so it never carries an opaque cursor either.
            cursor: None,
            count: None,
            options: None,
            local: None,
            pick: query.pick,
            omit: query.omit,
            ordering: query.ordering.unwrap_or_default(),
            scope_q: query.scope_q,
            lang: query.lang.map(LanguageFilter::from_tag),
            dataset_id: query.dataset_id,
            expand_values: query.expand_values,
            json_keys: query.json_keys,
            format: None,
            join: query.join,
            join_level: query.join_level,
            contained_by: query.contained_by,
            entity_map: query.entity_map,
            entity_map_lifetime: query.entity_map_lifetime,
            split_entities: query.split_entities,
            geometry_property: None,
            csf: query.csf,
            drop: None,
            keep: None,
            delete_all: None,
        };
        // Table 5.2.23-1 restricts the `Query` datatype's `attrs` member to
        // "Attribute name as short hand strings or URIs", where `pick` and
        // `omit` beside it take an "Entity member ("id", "type", "scope" or a
        // projected Attribute name)". A body without a `temporalQ` is answered
        // in an Entity representation, and clause 4.18 leaves the Scope
        // non-reified there, so `scope` is no Attribute name to give `attrs`.
        validate_attrs(parameters.attrs.as_deref(), AttrsAdmission::AttributeNamesOnly)?;
        Ok(parameters)
    }
}

#[cfg(test)]
mod tests {
    use crate::query::entity::parameters::EntityQueryParameters;
    use andromeda_ngsi_ld_model::query::body::Query;
    use andromeda_ngsi_ld_query::projection::grammar::RawProjectionTerm;

    #[test]
    fn every_applicable_query_member_survives_body_conversion() {
        let query: Query = serde_json::from_value(serde_json::json!({
            "type": "Query",
            "entities": [{"type": "Vehicle", "id": ["urn:ngsi-ld:Vehicle:1"]}],
            "q": "speed>50",
            "scopeQ": "/Madrid",
            "attrs": ["speed"],
            "pick": ["id", "speed"],
            "omit": ["scope"],
            "csf": "location==Madrid",
            "containedBy": ["urn:ngsi-ld:Building:1"],
            "datasetId": ["https://example.com/datasets/one", "@none"],
            "expandValues": "speed",
            "entityMap": true,
            "entityMapLifetime": "PT1H",
            "jsonKeys": "payload",
            "join": "inline",
            "joinLevel": 2,
            "lang": "de-DE",
            "ordering": {"orderBy": ["speed;desc"]},
            "splitEntities": true
        }))
        .expect("valid Query");

        let parameters = EntityQueryParameters::try_from(query).expect("entity-applicable query");
        pretty_assertions::assert_eq!(parameters.entity_selectors.len(), 1);
        pretty_assertions::assert_eq!(parameters.id.as_deref().map(|ids| ids[0].as_str()), Some("urn:ngsi-ld:Vehicle:1"));
        pretty_assertions::assert_eq!(parameters.entity_type.as_ref().map(ToString::to_string).as_deref(), Some("Vehicle"));
        pretty_assertions::assert_eq!(parameters.ordering.order_by.len(), 1);
        pretty_assertions::assert_eq!(parameters.dataset_id.as_ref().map(Vec::len), Some(2));
        assert!(parameters.scope_q.is_some());
        assert!(parameters.csf.is_some());
        assert!(parameters.entity_map_lifetime.is_some());
        assert!(parameters.lang.is_some());
    }

    #[test]
    fn multiple_body_selectors_are_not_flattened_into_a_cross_product() {
        let query: Query = serde_json::from_value(serde_json::json!({
            "type": "Query",
            "entities": [
                {"type": "Vehicle", "id": "urn:ngsi-ld:Vehicle:1"},
                {"type": "Building", "id": "urn:ngsi-ld:Building:1"}
            ]
        }))
        .expect("valid Query");
        let parameters = EntityQueryParameters::try_from(query).expect("entity-applicable query");
        assert!(parameters.id.is_none());
        assert!(parameters.entity_type.is_none());
        pretty_assertions::assert_eq!(parameters.entity_selectors.len(), 2);
    }

    /// Clause 5.2.23: `pick`/`omit` are `String[]`, each member its own
    /// clause 4.21 projection-language string. Every array member is parsed
    /// on its own and the resulting terms concatenate in array order.
    #[test]
    fn each_pick_array_member_parses_through_the_projection_grammar_and_concatenates() {
        let query: Query = serde_json::from_value(serde_json::json!({
            "type": "Query",
            "pick": ["observation{temperature}", "name"]
        }))
        .expect("valid Query");

        let parameters = EntityQueryParameters::try_from(query).expect("entity-applicable query");
        let terms = parameters.pick.expect("pick present");
        pretty_assertions::assert_eq!(terms.len(), 2);
        pretty_assertions::assert_eq!(terms[0].name.as_str(), "observation");
        pretty_assertions::assert_eq!(terms[0].linked.iter().map(|term| term.name.as_str()).collect::<Vec<_>>(), vec!["temperature"]);
        pretty_assertions::assert_eq!(terms[1].name.as_str(), "name");
        assert_eq!(terms[1].linked, [] as [RawProjectionTerm; 0]);
    }

    #[test]
    fn temporal_members_are_rejected_for_an_entity_query() {
        let query: Query = serde_json::from_value(serde_json::json!({
            "type": "Query",
            "temporalQ": {"timerel": "after", "timeAt": "2024-01-01T00:00:00Z"}
        }))
        .expect("valid canonical Query");
        assert!(EntityQueryParameters::try_from(query).is_err());
    }
}
