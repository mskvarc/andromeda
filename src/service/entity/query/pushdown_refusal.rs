//! Refusing a query the store cannot narrow at all.
//!
//! A residual filter is a correctness device and a performance cliff at the
//! same time. Where the store still cut the candidate set down, finishing the
//! predicate in memory is a refinement and is worth doing. Where it could not
//! narrow at all, finishing in memory means reading every entity the broker
//! holds, and clause 5.5.2 provides `OperationNotSupported` precisely so that
//! answer can be refused instead of given slowly and silently.

use crate::{core::error::AndromedaError, service::entity::storage_profile::StorageProfile};
use andromeda_ngsi_ld_query::entity_selection::EntitySelection;

/// Refuse a selection carrying a predicate the store cannot narrow.
///
/// # Errors
///
/// Returns [`AndromedaError::operation_not_supported`] naming every query
/// parameter the store could not narrow (clause 5.5.2).
pub(super) fn refuse_unnarrowable(profile: &StorageProfile, selection: &EntitySelection) -> Result<(), AndromedaError> {
    let refused = profile.unnarrowable(selection);
    if refused.is_empty() {
        return Ok(());
    }
    let named: Vec<String> = refused.iter().map(ToString::to_string).collect();
    Err(AndromedaError::operation_not_supported(format!(
        "this broker's storage backend cannot evaluate {}; the request would have to be answered by reading every entity",
        named.join(", ")
    )))
}

#[cfg(test)]
mod tests {
    use crate::service::entity::{
        query::pushdown_refusal::refuse_unnarrowable,
        storage_profile::{GeoPredicatePushdown, ScopePushdown, StorageProfile, ValueFilterPushdown},
    };
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry};
    use andromeda_ngsi_ld_query::{entity_selection::EntitySelection, filter::parser, geo::query::GeoQuery, scope::query::ScopeQuery};
    use serde_json::json;

    fn all_three() -> EntitySelection {
        EntitySelection::builder()
            .q_expr(Some(parser::parse("\"https://example.com/temperature\">25").expect("valid q filter")))
            .geo(Some(GeoQuery {
                georel: GeoRel::Near {
                    max_distance: Some(1000.0),
                    min_distance: None,
                },
                geometry: Geometry::from_geojson_value(&json!({"type": "Point", "coordinates": [13.4, 52.5]})).expect("valid geometry"),
                geoproperty_iri: "https://uri.etsi.org/ngsi-ld/location".parse().expect("valid IRI"),
            }))
            .scope_q(Some(ScopeQuery::parse("/A/#").expect("valid scopeQ")))
            .build()
    }

    #[test]
    fn a_query_a_store_cannot_narrow_is_refused_rather_than_table_scanned() {
        // Clause 5.5.2 provides OperationNotSupported for exactly this.
        let profile = StorageProfile {
            geo_predicates: GeoPredicatePushdown::None,
            value_filter: ValueFilterPushdown::JsonPath,
            scope: ScopePushdown::LabelTree,
        };
        let error = refuse_unnarrowable(&profile, &all_three()).expect_err("must refuse");
        assert!(error.to_string().contains("georel"), "the refusal names the parameter: {error}");
    }

    #[test]
    fn the_refusal_names_every_predicate_the_store_could_not_narrow() {
        let profile = StorageProfile {
            geo_predicates: GeoPredicatePushdown::None,
            value_filter: ValueFilterPushdown::None,
            scope: ScopePushdown::None,
        };
        let error = refuse_unnarrowable(&profile, &all_three()).expect_err("must refuse");
        let message = error.to_string();
        for parameter in ["q", "georel", "scopeQ"] {
            assert!(message.contains(parameter), "the refusal names {parameter}: {message}");
        }
    }

    #[test]
    fn a_query_a_store_can_narrow_partly_is_not_refused() {
        // A bounding box, a materialised column and a literal prefix each cut
        // the candidate set down, so the residual pass is a refinement.
        let profile = StorageProfile {
            geo_predicates: GeoPredicatePushdown::BoundingBoxOnly,
            value_filter: ValueFilterPushdown::Structured,
            scope: ScopePushdown::PrefixOnly,
        };
        assert!(refuse_unnarrowable(&profile, &all_three()).is_ok());
    }

    #[test]
    fn a_query_a_store_applies_fully_is_not_refused() {
        assert!(refuse_unnarrowable(&StorageProfile::POSTGIS_LTREE, &all_three()).is_ok());
    }
}
