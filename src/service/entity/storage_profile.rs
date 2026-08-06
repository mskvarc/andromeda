//! Which predicates a store can push down, and how far.
//!
//! Everything above a store port that would otherwise assume a PostgreSQL
//! property reads this instead. Each axis is a named enum rather than a
//! boolean, because "can it do geo" has three useful answers and only one of
//! them is a yes.
//!
//! Two questions are asked of it, from opposite sides of the port. The store
//! asks [`StorageProfile::unapplied`] to find out what it must hand back; the
//! policy asks [`StorageProfile::unnarrowable`] to find out what it should
//! refuse outright rather than finish over a full scan.
//!
//! These three axes are not entity-only, even though the only port that returns
//! a profile today is the entity store. `CompiledTemporalQuery` carries the
//! same [`EntitySelection`], and the temporal statement builder renders `q`,
//! `georel` and `scopeQ` from it against `attribute_history` and
//! `scope_history`. So a temporal store that is not PostgreSQL answers exactly
//! these three questions too. The temporal port gains a profile at step 11 of
//! `docs/workspace-topology.md`, which is what first gives it one to return.
//!
//! [`EntitySelection`]: andromeda_ngsi_ld_query::entity_selection::EntitySelection
//! [`StorageProfile::unapplied`]: crate::service::entity::storage_profile::StorageProfile::unapplied
//! [`StorageProfile::unnarrowable`]: crate::service::entity::storage_profile::StorageProfile::unnarrowable

use crate::service::entity::residual_filter::{ResidualFilter, ResidualPredicate};
use andromeda_ngsi_ld_query::entity_selection::EntitySelection;

/// How far a store can evaluate a clause 4.10 geo-query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeoPredicatePushdown {
    /// Every `georel` the specification defines, exactly.
    Full,
    /// Only a bounding-box intersection, which narrows the candidate set but is
    /// not the relation clause 4.10 asks for.
    // Constructed by no adapter yet. It describes a store with coordinate
    // comparisons but no topology -- ClickHouse has `greatCircleDistance` and
    // `pointInPolygon` and no PostGIS -- and that store is the temporal one, so
    // it cannot be written before step 11 gives the temporal read path a port.
    #[allow(dead_code)]
    BoundingBoxOnly,
    /// No geospatial predicate at all.
    None,
}

/// How far a store can evaluate a clause 4.9 `q` filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueFilterPushdown {
    /// Arbitrary attribute paths into the stored document.
    JsonPath,
    /// Only predicates over columns the store materialised in advance, so a
    /// path it did not anticipate cannot be answered.
    // Constructed by no adapter yet, for the reason on
    // [`GeoPredicatePushdown::BoundingBoxOnly`].
    #[allow(dead_code)]
    Structured,
    /// No value predicate at all.
    None,
}

/// How far a store can evaluate a clause 4.18 `scopeQ`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopePushdown {
    /// The full path language, including the `+` and `#` wildcards.
    LabelTree,
    /// Only a literal prefix, so a wildcard in a non-final position narrows but
    /// does not decide.
    // Constructed by no adapter yet, for the reason on
    // [`GeoPredicatePushdown::BoundingBoxOnly`].
    #[allow(dead_code)]
    PrefixOnly,
    /// No scope predicate at all.
    None,
}

/// What one store can be asked to do with a compiled query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageProfile {
    /// Clause 4.10 geo-queries.
    pub geo_predicates: GeoPredicatePushdown,
    /// Clause 4.9 `q` filters.
    pub value_filter: ValueFilterPushdown,
    /// Clause 4.18 `scopeQ` expressions.
    pub scope: ScopePushdown,
}

impl StorageProfile {
    /// A PostgreSQL store with PostGIS geometry columns, `jsonb` payloads and
    /// `ltree` scope paths.
    ///
    /// Constant rather than probed, which is the difference between these axes
    /// and the TimescaleDB edition. `migrations/tenant/` creates the PostGIS
    /// and `ltree` columns, so a pool that migrated has them and one that could
    /// not migrate never becomes a store. There is no runtime state to detect
    /// and no per-tenant divergence to carry.
    pub const POSTGIS_LTREE: StorageProfile = StorageProfile {
        geo_predicates: GeoPredicatePushdown::Full,
        value_filter: ValueFilterPushdown::JsonPath,
        scope: ScopePushdown::LabelTree,
    };

    /// The predicates of `selection` this store cannot apply exactly.
    ///
    /// A store that narrows a predicate without deciding it — a bounding box
    /// standing in for a `georel`, a prefix standing in for a wildcard path —
    /// still hands it back: the rows it returns are a superset, and returning a
    /// superset without saying so is the failure this whole mechanism exists to
    /// prevent.
    pub fn unapplied(&self, selection: &EntitySelection) -> ResidualFilter {
        let mut residual = EntitySelection::default();
        if selection.geo.is_some() && self.geo_predicates != GeoPredicatePushdown::Full {
            // clone: the residual owns the predicate the store keeps for its
            // own narrowing pass
            residual.geo = selection.geo.clone();
        }
        if selection.q_expr.is_some() && self.value_filter != ValueFilterPushdown::JsonPath {
            residual.q_expr = selection.q_expr.clone(); // clone: as above
        }
        if selection.scope_q.is_some() && self.scope != ScopePushdown::LabelTree {
            residual.scope_q = selection.scope_q.clone(); // clone: as above
        }
        ResidualFilter::over(residual)
    }

    /// The predicates of `selection` this store cannot narrow at all.
    ///
    /// Finishing one of these in memory means reading every entity the broker
    /// holds. Clause 5.5.2 provides `OperationNotSupported` for exactly this,
    /// and answering slowly and silently is the alternative it exists to
    /// replace.
    pub fn unnarrowable(&self, selection: &EntitySelection) -> Vec<ResidualPredicate> {
        let mut refused = Vec::new();
        if selection.q_expr.is_some() && self.value_filter == ValueFilterPushdown::None {
            refused.push(ResidualPredicate::ValueFilter);
        }
        if selection.geo.is_some() && self.geo_predicates == GeoPredicatePushdown::None {
            refused.push(ResidualPredicate::GeoQuery);
        }
        if selection.scope_q.is_some() && self.scope == ScopePushdown::None {
            refused.push(ResidualPredicate::ScopeQuery);
        }
        refused
    }
}

#[cfg(test)]
mod tests {
    use crate::service::entity::{
        residual_filter::ResidualPredicate,
        storage_profile::{GeoPredicatePushdown, ScopePushdown, StorageProfile, ValueFilterPushdown},
    };
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry};
    use andromeda_ngsi_ld_query::{entity_selection::EntitySelection, filter::parser, geo::query::GeoQuery, scope::query::ScopeQuery};
    use serde_json::json;

    fn profile(geo: GeoPredicatePushdown, value_filter: ValueFilterPushdown, scope: ScopePushdown) -> StorageProfile {
        StorageProfile {
            geo_predicates: geo,
            value_filter,
            scope,
        }
    }

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

    /// A selection carrying a `q`, a `geoQ` and a `scopeQ`.
    fn all_three() -> EntitySelection {
        EntitySelection::builder()
            .q_expr(Some(parser::parse("\"https://example.com/temperature\">25").expect("valid q filter")))
            .geo(Some(geo_query()))
            .scope_q(Some(ScopeQuery::parse("/A/#").expect("valid scopeQ")))
            .build()
    }

    #[test]
    fn a_store_that_pushes_everything_down_leaves_no_residual() {
        assert!(StorageProfile::POSTGIS_LTREE.unapplied(&all_three()).is_empty());
    }

    #[test]
    fn a_store_with_no_geo_support_hands_the_geo_predicate_back() {
        // Clause 4.10 cannot be evaluated by a store with no geometry support.
        let profile = profile(GeoPredicatePushdown::None, ValueFilterPushdown::JsonPath, ScopePushdown::LabelTree);
        pretty_assertions::assert_eq!(profile.unapplied(&all_three()).kinds(), vec![ResidualPredicate::GeoQuery]);
    }

    #[test]
    fn a_store_with_bounding_box_geo_still_hands_the_exact_relation_back() {
        // A bounding box narrows the candidate set; it is not the clause 4.10
        // relation, so the exact predicate is still outstanding.
        let profile = profile(GeoPredicatePushdown::BoundingBoxOnly, ValueFilterPushdown::JsonPath, ScopePushdown::LabelTree);
        pretty_assertions::assert_eq!(profile.unapplied(&all_three()).kinds(), vec![ResidualPredicate::GeoQuery]);
    }

    #[test]
    fn a_store_with_no_value_filter_support_hands_the_q_filter_back() {
        // Clause 4.9 over a store that cannot address the stored document.
        let profile = profile(GeoPredicatePushdown::Full, ValueFilterPushdown::None, ScopePushdown::LabelTree);
        pretty_assertions::assert_eq!(profile.unapplied(&all_three()).kinds(), vec![ResidualPredicate::ValueFilter]);
    }

    #[test]
    fn a_store_with_no_scope_support_hands_the_scope_query_back() {
        // Clause 4.18 over a store with no path language.
        let profile = profile(GeoPredicatePushdown::Full, ValueFilterPushdown::JsonPath, ScopePushdown::None);
        pretty_assertions::assert_eq!(profile.unapplied(&all_three()).kinds(), vec![ResidualPredicate::ScopeQuery]);
    }

    #[test]
    fn a_store_hands_back_only_the_predicates_the_query_actually_carries() {
        // Clause 5.7.2: an axis says what the store can do, not what the query
        // asked for, so an unsupported axis with no matching predicate is not
        // a residual.
        let profile = profile(GeoPredicatePushdown::None, ValueFilterPushdown::None, ScopePushdown::None);
        let selection = EntitySelection::builder()
            .q_expr(Some(parser::parse("\"https://example.com/temperature\">25").expect("valid q filter")))
            .build();
        pretty_assertions::assert_eq!(profile.unapplied(&selection).kinds(), vec![ResidualPredicate::ValueFilter]);
    }

    #[test]
    fn a_store_that_cannot_narrow_a_predicate_at_all_names_it_as_unnarrowable() {
        // Clause 5.5.2: finishing this in memory means reading every entity.
        let profile = profile(GeoPredicatePushdown::None, ValueFilterPushdown::JsonPath, ScopePushdown::LabelTree);
        pretty_assertions::assert_eq!(profile.unnarrowable(&all_three()), vec![ResidualPredicate::GeoQuery]);
    }

    #[test]
    fn a_store_that_narrows_a_predicate_partly_does_not_name_it_as_unnarrowable() {
        // The store still cut the candidate set down, so the residual pass is a
        // refinement rather than a table scan and clause 5.5.2 does not apply.
        let profile = profile(
            GeoPredicatePushdown::BoundingBoxOnly,
            ValueFilterPushdown::Structured,
            ScopePushdown::PrefixOnly,
        );
        let selection = all_three();
        assert!(profile.unnarrowable(&selection).is_empty());
        assert!(!profile.unapplied(&selection).is_empty());
    }

    #[test]
    fn the_postgresql_profile_narrows_and_applies_every_predicate() {
        let selection = all_three();
        assert!(StorageProfile::POSTGIS_LTREE.unnarrowable(&selection).is_empty());
        assert!(StorageProfile::POSTGIS_LTREE.unapplied(&selection).is_empty());
    }
}
