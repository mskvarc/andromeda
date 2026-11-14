//! Filters held back from the local query when `splitEntities` is in effect.

use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::{filter::compilation::AdmittedFilter, geo::input::GeoQueryInput, scope::query::ScopeQuery};

/// The `q`, `geoQ` and `scopeQ` parameters a `splitEntities` request removes
/// from the query it runs locally, so they can be evaluated once against the
/// merged local-plus-forwarded result instead (NGSI-LD 5.7.2.4 for entities,
/// 5.7.4.4 for temporal entities).
///
/// Applying them before the merge would drop entities whose attributes are
/// split across Context Sources: each source sees only its own fragment, so a
/// predicate over an attribute another source holds cannot be satisfied there.
///
/// [`Default`] is the not-deferring case (every field `None`), which is what a
/// request without `splitEntities` needs.
#[derive(Debug, Default)]
pub struct DeferredSplitFilters {
    /// The deferred `q`, as the request's validation admitted it.
    pub q: Option<AdmittedFilter>,
    /// Deferred complete geo-query.
    pub geo: Option<GeoQueryInput>,
    /// Deferred `scopeQ` scope query.
    pub scope_q: Option<ScopeQuery>,
    /// The request's `jsonKeys` declaration, which travels with the deferred
    /// `q` because it decides how that filter's terms expand.
    ///
    /// Clause 4.9 EXAMPLE 11: with
    /// `?q=parkingTickets[value]=="Overstay 60 minutes"&jsonKeys=parkingTickets`
    /// "the target value raw is JSON, and is not expanded to ngsi-ld:hasValue
    /// using the core @context". Expanding the deferred filter without the
    /// declaration would address `ngsi-ld:hasValue` where the entity holds a
    /// literal `value` key, and every entity the client asked for would be
    /// dropped after the merge.
    pub json_keys: Option<Vec<AttributeName>>,
    /// The request's `expandValues` list, which travels with the deferred `q`
    /// because it decides which operands are terms to expand, and so where a
    /// bare word may stand (clause 4.9 EXAMPLE 12).
    pub expand_values: Option<Vec<AttributeName>>,
}

#[cfg(test)]
mod tests {
    use crate::query::{
        entity::{admitted::AdmittedEntityQuery, operation::EntityQueryOperation, parameters::EntityQueryParameters},
        permissive_admission::PERMISSIVE,
    };
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_model::query::body::Query;
    use serde_json::json;

    /// The query a clause 5.2.23 body describes, which is the cheapest way to
    /// build a complete set of parameters, admitted for Query Entities.
    fn query_of(body: serde_json::Value) -> AdmittedEntityQuery {
        let body: Query = serde_json::from_value(body).expect("a valid clause 5.2.23 body");
        EntityQueryParameters::try_from(body)
            .expect("an entity-applicable query")
            .admit(EntityQueryOperation::Query, PERMISSIVE)
            .expect("a query validation admits")
    }

    /// Clause 4.9 EXAMPLE 11: with
    /// `?q=parkingTickets[value]=="Overstay 60 minutes"&jsonKeys=parkingTickets`
    /// "the target value raw is JSON, and is not expanded to ngsi-ld:hasValue
    /// using the core @context". The declaration decides how the deferred
    /// filter's own terms expand, so it has to travel with the filter: expanded
    /// without it, the residual filter addresses `ngsi-ld:hasValue` where the
    /// entity holds a literal `value` key and drops every entity that matches.
    #[test]
    fn the_json_keys_declaration_travels_with_the_filter_it_governs() {
        let mut query = query_of(json!({
            "type": "Query",
            "q": "parkingTickets[value]==\"Overstay 60 minutes\"",
            "jsonKeys": "parkingTickets"
        }));

        let deferred = query.defer_split_filters();

        assert!(deferred.q.is_some(), "the filter itself is deferred");
        pretty_assertions::assert_eq!(
            deferred
                .json_keys
                .as_deref()
                .unwrap_or_default()
                .iter()
                .map(AttributeName::as_str)
                .collect::<Vec<_>>(),
            vec!["parkingTickets"]
        );
        assert!(
            query.json_keys.is_none(),
            "with `q` deferred the declaration has nothing left to say to the local query"
        );
        assert!(query.q.is_none() && query.filter().is_none(), "the admission leaves with the filter it covers");
    }

    /// A query that declared nothing defers nothing, and the expansion of its
    /// deferred filter is the one every filter without a declaration gets.
    #[test]
    fn a_query_without_a_declaration_defers_none() {
        let mut query = query_of(json!({"type": "Query", "q": "speed>50"}));

        let deferred = query.defer_split_filters();

        assert!(deferred.q.is_some());
        assert!(deferred.json_keys.is_none());
    }

    /// Clause 5.7.2.4: the geoquery and `scopeQ` are held back beside `q`, and a
    /// query that carried them leaves the local read selecting on identity alone.
    #[test]
    fn the_geoquery_and_scope_query_are_deferred_beside_q() {
        let mut query = query_of(json!({
            "type": "Query",
            "geoQ": {"georel": "near;maxDistance==2000", "geometry": "Point", "coordinates": [13.4, 52.5]},
            "scopeQ": "/Madrid/Gardens"
        }));

        let deferred = query.defer_split_filters();

        assert!(deferred.geo.is_some(), "the geoquery is deferred");
        assert!(deferred.scope_q.is_some(), "the scope query is deferred");
        assert!(query.geo.0.is_none(), "the local query keeps no geoquery");
        assert!(query.scope_q.is_none(), "the local query keeps no scope query");
    }
}
