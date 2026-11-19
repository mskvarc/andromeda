//! The query a periodic (`timeInterval`) notification reads its candidates
//! with (clause 5.8.6).
//!
//! When the interval is reached, clause 5.8.6 sends "all the subscribed
//! Entities that match the query, geoquery and Scope query conditions". The
//! entity selectors, the `q`, the `geoQ` and the `scopeQ` are the members a
//! Query Entities request carries, so the store reads the candidates through
//! the same query engine, and only the Entities its filters keep are read at
//! all. The evaluation in memory decides them again, at the same instant.

use andromeda_ngsi_ld_model::entity_selector::EntitySelector as StoredSelector;
use andromeda_ngsi_ld_query::{
    entity::compiled::CompiledEntityQuery,
    entity_selection::EntitySelection,
    expanded_entity_selector::ExpandedEntitySelector,
    query_error::QueryError,
    standing_instant::StandingInstant,
};
use andromeda_subscription::{compiled::CompiledSubscription, entity_filter_set::EntityFilterSet};
use chrono::{DateTime, Utc};

/// The query reading the Entities `sub` notifies of as they stand at `at`:
/// its selectors as a union (clause 5.2.33) under its `q`, `geoQ` and
/// `scopeQ`, every Entity when it names none of them, and `None` when a
/// stored `geoQ` that no longer compiles admits no Entity.
///
/// # Errors
///
/// Returns [`QueryError::TermExpansion`] when a stored selector's type is no
/// expanded IRI.
pub(crate) fn interval_query(sub: &CompiledSubscription, at: DateTime<Utc>) -> Result<Option<CompiledEntityQuery>, QueryError> {
    let filters = match sub.entity_filters() {
        EntityFilterSet::AdmitsNothing => return Ok(None),
        EntityFilterSet::Evaluate(filters) => filters,
    };
    // clone: the query owns its filters, and the subscription keeps its own for the evaluation in memory
    let selection = EntitySelection::builder()
        .selectors(sub.selectors.iter().map(compiled_selector).collect::<Result<_, _>>()?)
        .q(filters.q.cloned())
        .geo(filters.geo.cloned())
        .scope_q(filters.scope.cloned())
        .standing(StandingInstant::At(at))
        .build();
    Ok(Some(CompiledEntityQuery::builder().selection(selection).build()))
}

/// A stored selector as a query compiles it, its type the expanded IRIs the
/// subscription stored.
fn compiled_selector(selector: &StoredSelector) -> Result<ExpandedEntitySelector, QueryError> {
    // clone: the query owns its selectors, and the subscription lends its own
    Ok(ExpandedEntitySelector::builder()
        .ids(selector.id.clone().unwrap_or_default())
        .id_pattern(selector.id_pattern.clone())
        .type_selection(Some(selector.type_.clone().into_expanded()?))
        .build())
}

#[cfg(test)]
mod tests {
    use crate::{
        interval_query::interval_query,
        periodic_fixture::{VEHICLE, periodic, tick},
    };
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_query::{
        entity::compiled::CompiledEntityQuery,
        name_stage::Expanded,
        projection::projected_members::ProjectedMembers,
        standing_instant::StandingInstant,
        type_selection::TypeSelection,
    };
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    fn query_of(members: &Value) -> CompiledEntityQuery {
        interval_query(&periodic(members), tick())
            .expect("the stored types are IRIs")
            .expect("the subscription admits entities")
    }

    /// The selectors of the query, as the identifiers and the type each
    /// names.
    fn selected(query: &CompiledEntityQuery) -> Vec<(Vec<Identifier>, Option<TypeSelection<Expanded>>)> {
        query
            .selection
            .selectors
            .iter()
            .map(|selector| (selector.ids.clone(), selector.type_selection.clone()))
            .collect()
    }

    fn vehicle_type() -> TypeSelection<Expanded> {
        TypeSelection::single(IriBuf::new(VEHICLE.to_owned()).expect("valid IRI").into())
    }

    /// Clause 5.2.33: the selectors are a union, each read as written, so an
    /// Entity two of them select is one candidate of the one query.
    #[test]
    fn two_selectors_are_read_as_one_union() {
        let query = query_of(&json!({"entities": [{"type": VEHICLE, "id": "urn:ngsi-ld:Vehicle:2"}, {"type": VEHICLE}]}));
        assert_eq!(
            selected(&query),
            vec![
                (vec!["urn:ngsi-ld:Vehicle:2".parse().expect("valid URI")], Some(vehicle_type())),
                (Vec::new(), Some(vehicle_type())),
            ]
        );
    }

    /// Clause 5.8.6: a stored `geoQ` that no longer compiles is still a
    /// defined geoquery, so no Entity is read rather than every one.
    #[test]
    fn a_geo_q_that_no_longer_compiles_reads_no_candidate() {
        let unusable = periodic(&json!({
            "entities": [{"type": VEHICLE}],
            "geoQ": {"georel": "near;maxDistance==2000", "geometry": "Point", "coordinates": "[1, "}
        }));
        assert!(interval_query(&unusable, tick()).expect("the stored types are IRIs").is_none());
    }

    /// Clause 5.8.6: "If none of query, geoquery and Scope query are defined,
    /// then all subscribed Entities shall be included", so the selectors alone
    /// decide, and nothing narrows what each Entity carries.
    #[test]
    fn a_subscription_without_filters_reads_its_selectors_alone() {
        let query = query_of(&json!({"entities": [{"type": VEHICLE}]}));
        assert_eq!(selected(&query), vec![(Vec::new(), Some(vehicle_type()))]);
        assert_eq!(
            (
                query.selection.q.is_none(),
                query.selection.geo.is_none(),
                query.selection.scope_q.is_none(),
                query.selection.attrs.is_empty(),
                query.selection.projected_members,
                query.selection.standing,
            ),
            (true, true, true, true, ProjectedMembers::Unconditional, StandingInstant::At(tick()))
        );
    }

    /// The query reads the Entities every filter keeps, judged at the tick's
    /// one instant.
    #[test]
    fn the_filters_are_read_with_the_selectors_at_the_tick_instant() {
        let query = query_of(&json!({
            "entities": [{"type": VEHICLE}],
            "q": "\"https://example.org/speed\">10",
            "geoQ": {"georel": "near;maxDistance==2000", "geometry": "Point", "coordinates": "[8.68, 49.40]"},
            "scopeQ": "/Spain"
        }));
        assert_eq!(
            (
                query.selection.q.is_some(),
                query.selection.geo.is_some(),
                query.selection.scope_q.is_some(),
                query.selection.standing,
            ),
            (true, true, true, StandingInstant::At(tick()))
        );
    }
}
