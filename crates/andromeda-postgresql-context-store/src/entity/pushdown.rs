//! Deciding what to ask PostgreSQL for, and what to hand back unanswered.
//!
//! The decision is read off this store's [`StorageProfile`] rather than
//! assumed. For a PostgreSQL store with PostGIS and `ltree` every axis is at
//! its full value, so nothing is ever handed back -- but the branch that would
//! is the same branch a store without those extensions takes, which is what
//! makes the residual machinery above the port real rather than notional.
//!
//! [`StorageProfile`]: crate::entity::storage_profile::StorageProfile

use crate::entity::{pagination_outcome::PaginationOutcome, query_outcome::QueryOutcome, residual_filter::ResidualFilter, storage_profile::StorageProfile};
use andromeda_ngsi_ld_query::{
    entity::compiled::CompiledEntityQuery,
    pagination::{CountPolicy, Pagination},
};
use std::borrow::Cow;

/// The statement this store will issue, and the predicates it will not.
pub(super) struct Pushdown<'a> {
    /// The query to render, with any predicate this store cannot apply removed.
    ///
    /// Borrowed in the ordinary case: nothing was removed, so nothing was
    /// copied.
    pub query: Cow<'a, CompiledEntityQuery>,
    /// The predicates the caller must finish.
    residual: ResidualFilter,
}

/// Split a compiled query into the part this store can answer and the part it
/// cannot.
pub(super) fn plan<'a>(compiled: &'a CompiledEntityQuery, profile: &StorageProfile) -> Pushdown<'a> {
    let residual = profile.unapplied(&compiled.selection);
    if residual.is_empty() {
        return Pushdown {
            query: Cow::Borrowed(compiled),
            residual,
        };
    }

    // clone: only when a predicate has to be stripped, which the PostgreSQL
    // profile never triggers
    let mut narrowed = compiled.clone();
    residual.strip_from(&mut narrowed.selection);
    // Clause 5.5.6: a window over rows a later filter will thin leaves the page
    // short and every later page misaligned, so it is not applied here at all.
    narrowed.pagination = Pagination {
        limit: u32::MAX,
        offset: 0,
        count: CountPolicy::Omit,
    };
    Pushdown {
        query: Cow::Owned(narrowed),
        residual,
    }
}

impl Pushdown<'_> {
    /// Wrap the rows this plan produced in the outcome it implies.
    pub(super) fn outcome<T>(self, rows: Vec<T>, total: Option<i64>) -> QueryOutcome<T> {
        let pagination = if self.residual.is_empty() {
            PaginationOutcome::Applied { total }
        } else {
            PaginationOutcome::Deferred
        };
        QueryOutcome {
            rows,
            unapplied: self.residual,
            pagination,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::{
        pagination_outcome::PaginationOutcome,
        pushdown::plan,
        residual_predicate::ResidualPredicate,
        storage_profile::{GeoPredicatePushdown, ScopePushdown, StorageProfile, ValueFilterPushdown},
    };
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry};
    use andromeda_ngsi_ld_query::{
        entity::compiled::CompiledEntityQuery,
        entity_selection::EntitySelection,
        filter::parser,
        geo::query::GeoQuery,
        pagination::{CountPolicy, Pagination},
        scope::query::ScopeQuery,
    };
    use serde_json::json;

    /// A geo-query centred on Berlin.
    fn geo_query() -> GeoQuery {
        GeoQuery {
            georel: GeoRel::Near {
                max_distance: Some(1000.0),
                min_distance: None,
            },
            geometry: Geometry::from_geojson_value(&json!({"type": "Point", "coordinates": [13.4, 52.5]})).expect("valid geometry"),
            geoproperty_iri: "https://uri.etsi.org/ngsi-ld/location".parse().expect("valid IRI"),
        }
    }

    /// A compiled query carrying all three predicate axes plus a window.
    fn query() -> CompiledEntityQuery {
        CompiledEntityQuery::builder()
            .selection(
                EntitySelection::builder()
                    .q_expr(Some(parser::parse("\"https://example.com/temperature\">25").expect("valid q filter")))
                    .geo(Some(geo_query()))
                    .scope_q(Some(ScopeQuery::parse("/A/#").expect("valid scopeQ")))
                    .build(),
            )
            .pagination(Pagination {
                limit: 20,
                offset: 5,
                count: CountPolicy::Emit,
            })
            .build()
    }

    #[test]
    fn a_complete_pushdown_issues_the_query_verbatim_and_reports_the_window_as_applied() {
        // Clause 6.3.13: the count the store produced is the answer.
        let compiled = query();
        let planned = plan(&compiled, &StorageProfile::POSTGIS_LTREE);
        pretty_assertions::assert_eq!(planned.query.pagination, compiled.pagination);
        assert!(planned.query.selection.geo.is_some());
        let outcome = planned.outcome(vec!["a"], Some(7));
        assert!(outcome.unapplied.is_empty());
        pretty_assertions::assert_eq!(outcome.pagination, PaginationOutcome::Applied { total: Some(7) });
    }

    #[test]
    fn an_incomplete_pushdown_strips_the_predicate_it_cannot_apply() {
        // Clause 4.10: a store with no geometry support must not be asked to
        // render a `georel`, and must say it did not apply one.
        let compiled = query();
        let profile = StorageProfile {
            geo_predicates: GeoPredicatePushdown::None,
            value_filter: ValueFilterPushdown::JsonPath,
            scope: ScopePushdown::LabelTree,
        };
        let planned = plan(&compiled, &profile);
        assert!(planned.query.selection.geo.is_none(), "the geo predicate is not sent");
        assert!(planned.query.selection.q_expr.is_some(), "the q filter still is");
        let outcome = planned.outcome(vec!["a"], Some(7));
        pretty_assertions::assert_eq!(outcome.unapplied.kinds(), vec![ResidualPredicate::GeoQuery]);
        pretty_assertions::assert_eq!(outcome.pagination, PaginationOutcome::Deferred);
    }

    /// Every combination of the three axes.
    fn every_profile() -> Vec<StorageProfile> {
        let mut profiles = Vec::new();
        for geo_predicates in [GeoPredicatePushdown::Full, GeoPredicatePushdown::BoundingBoxOnly, GeoPredicatePushdown::None] {
            for value_filter in [ValueFilterPushdown::JsonPath, ValueFilterPushdown::Structured, ValueFilterPushdown::None] {
                for scope in [ScopePushdown::LabelTree, ScopePushdown::PrefixOnly, ScopePushdown::None] {
                    profiles.push(StorageProfile {
                        geo_predicates,
                        value_filter,
                        scope,
                    });
                }
            }
        }
        profiles
    }

    #[test]
    fn what_a_plan_strips_is_exactly_what_the_profile_handed_back() {
        // `unapplied` and `strip_from` walk the same seven members from opposite
        // ends. A predicate named but not stripped is sent to a store that has
        // already said it cannot apply it; one stripped but not named is dropped
        // from the query and never finished by anyone.
        let compiled = query();
        for profile in every_profile() {
            let residual = profile.unapplied(&compiled.selection);
            let planned = plan(&compiled, &profile);
            let kinds = residual.kinds();

            pretty_assertions::assert_eq!(
                planned.query.selection.q_expr.is_none(),
                kinds.contains(&ResidualPredicate::ValueFilter),
                "q stripped exactly when handed back, for {profile:?}"
            );
            pretty_assertions::assert_eq!(
                planned.query.selection.geo.is_none(),
                kinds.contains(&ResidualPredicate::GeoQuery),
                "georel stripped exactly when handed back, for {profile:?}"
            );
            pretty_assertions::assert_eq!(
                planned.query.selection.scope_q.is_none(),
                kinds.contains(&ResidualPredicate::ScopeQuery),
                "scopeQ stripped exactly when handed back, for {profile:?}"
            );
        }
    }

    #[test]
    fn a_plan_that_hands_anything_back_never_also_claims_the_window() {
        // Clause 5.5.6: the two are mutually exclusive, and the policy layer
        // refuses the combination as a broken store rather than compensating.
        let compiled = query();
        for profile in every_profile() {
            let planned = plan(&compiled, &profile);
            let pagination = planned.query.pagination;
            let outcome = planned.outcome(vec!["a"], Some(7));
            match outcome.pagination {
                PaginationOutcome::Applied { total: _ } => {
                    assert!(outcome.unapplied.is_empty(), "an applied window leaves no residual, for {profile:?}");
                    pretty_assertions::assert_eq!(pagination, compiled.pagination, "a complete plan issues the caller's window, for {profile:?}");
                }
                PaginationOutcome::Deferred => {
                    assert!(
                        !outcome.unapplied.is_empty(),
                        "a deferred window means something was handed back, for {profile:?}"
                    );
                    pretty_assertions::assert_eq!(pagination.offset, 0, "a deferred plan skips nothing, for {profile:?}");
                    pretty_assertions::assert_eq!(pagination.limit, u32::MAX, "a deferred plan takes everything, for {profile:?}");
                    pretty_assertions::assert_eq!(pagination.count, CountPolicy::Omit, "a deferred plan counts nothing, for {profile:?}");
                }
            }
        }
    }

    #[test]
    fn an_incomplete_pushdown_takes_no_window_and_asks_for_no_count() {
        // Clause 5.5.6: the caller windows what survives the residual, so the
        // store must return every candidate row and count nothing.
        let compiled = query();
        let profile = StorageProfile {
            geo_predicates: GeoPredicatePushdown::None,
            value_filter: ValueFilterPushdown::JsonPath,
            scope: ScopePushdown::LabelTree,
        };
        let planned = plan(&compiled, &profile);
        pretty_assertions::assert_eq!(planned.query.pagination.offset, 0);
        pretty_assertions::assert_eq!(planned.query.pagination.limit, u32::MAX);
        pretty_assertions::assert_eq!(planned.query.pagination.count, CountPolicy::Omit);
    }
}
