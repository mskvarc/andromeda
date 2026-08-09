//! Conversion of the canonical clause-5.2.23 body into entity-query parameters.

use crate::{
    attribute_selection::validate_attrs,
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

        let entity_selectors = query
            .entities
            .unwrap_or_default()
            .into_iter()
            .map(|selector| EntitySelectorParams {
                id: selector.id,
                id_pattern: selector.id_pattern,
                type_: Some(selector.type_),
            })
            .collect();

        let parameters = EntityQueryParameters {
            id: None,
            entity_type: None,
            id_pattern: None,
            entity_selectors,
            attrs: query.attrs,
            q: query.q,
            geo: OptionalGeoQuery(query.geo_q),
            limit: None,
            offset: None,
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
        };
        validate_attrs(parameters.attrs.as_deref())?;
        Ok(parameters)
    }
}

#[cfg(test)]
mod tests {
    use crate::query::entity::parameters::EntityQueryParameters;
    use andromeda_ngsi_ld_model::query::body::Query;

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
        pretty_assertions::assert_eq!(parameters.ordering.order_by.len(), 1);
        pretty_assertions::assert_eq!(parameters.dataset_id.as_ref().map(Vec::len), Some(2));
        assert!(parameters.scope_q.is_some());
        assert!(parameters.csf.is_some());
        assert!(parameters.entity_map_lifetime.is_some());
        assert!(parameters.lang.is_some());
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
