//! What a subscription's `q`, `geoQ` and `scopeQ` leave to be evaluated over an entity.

use andromeda_ngsi_ld_query::entity_filters::EntityFilters;

/// The filters an entity is judged by (clause 5.8.6), or the fact that none can be evaluated.
#[derive(Debug)]
pub enum EntityFilterSet<'a> {
    /// The stored `geoQ` does not compile, and a defined geoquery the broker
    /// cannot evaluate admits no entity.
    AdmitsNothing,
    /// The filters to evaluate; a subscription without any admits every entity.
    Evaluate(EntityFilters<'a>),
}
