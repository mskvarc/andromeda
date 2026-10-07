//! Which predicates a store can push down, and how far.
//!
//! Everything above a store port that would otherwise assume a PostgreSQL
//! property reads this instead. Each axis is a named enum rather than a
//! boolean, because "can it do geo" has three useful answers and only one of
//! them is a yes.

use crate::{query_residual::ResidualFilter, residual_predicate::ResidualPredicate};
use andromeda_ngsi_ld_query::entity_selection::EntitySelection;

/// How far a store can evaluate a clause 4.10 geo-query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeoPredicatePushdown {
    /// Every `georel` the specification defines, exactly.
    Full,
    /// Only a bounding-box intersection, which narrows the candidate set but is
    /// not the relation clause 4.10 asks for.
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
    /// A PostgreSQL store with `PostGIS` geometry columns, `jsonb` payloads and
    /// `ltree` scope paths.
    ///
    /// Constant rather than probed, which is the difference between these axes
    /// and the `TimescaleDB` edition. The tenant migrations create the `PostGIS`
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
    /// A store that narrows a predicate without deciding it (a bounding box
    /// standing in for a `georel`, a prefix standing in for a wildcard path)
    /// still hands it back: the rows it returns are a superset, and returning a
    /// superset without saying so is the failure this whole mechanism exists to
    /// prevent.
    #[must_use]
    pub fn unapplied(&self, selection: &EntitySelection) -> ResidualFilter {
        let mut residual = EntitySelection::default();
        if selection.geo.is_some() && self.geo_predicates != GeoPredicatePushdown::Full {
            // clone: the residual owns the predicate the store keeps for its
            // own narrowing pass
            residual.geo.clone_from(&selection.geo);
        }
        if selection.q_expr.is_some() && self.value_filter != ValueFilterPushdown::JsonPath {
            residual.q_expr.clone_from(&selection.q_expr); // clone: as above
        }
        if selection.scope_q.is_some() && self.scope != ScopePushdown::LabelTree {
            residual.scope_q.clone_from(&selection.scope_q); // clone: as above
        }
        ResidualFilter::over(residual)
    }

    /// The predicates of `selection` this store cannot narrow at all.
    ///
    /// Finishing one of these in memory means reading every entity the broker
    /// holds. Clause 5.5.2 provides `OperationNotSupported` for exactly this,
    /// and answering slowly and silently is the alternative it exists to
    /// replace.
    #[must_use]
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
