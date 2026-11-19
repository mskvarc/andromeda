//! The value, geo and scope filters that select one entity.
//!
//! The `q` filter (clause 4.9), the geo-query (clause 4.10) and the scope
//! query (clause 4.19) select an entity together: the entities a query
//! answers with match all three it carries (clause 5.7.2.4), and a
//! Subscription notifies of an entity that matches its query, geoquery and
//! scopeQuery (clause 5.8.6). The record borrows the three, compiled, from
//! wherever they are held, so an entity query, a split query and a
//! Subscription hand the same filters to one admission.

use crate::{
    entity_selection::EntitySelection,
    filter::compiled_filter::CompiledFilter,
    geo::compiled_geo_query::CompiledGeoQuery,
    scope::scope_query::ScopeQuery,
};

/// The value, geo and scope filters an entity is selected by, each absent when
/// the query or Subscription carries none.
#[derive(Debug, Clone, Copy)]
pub struct EntityFilters<'q> {
    /// The `q` filter (clause 4.9), compiled.
    pub q: Option<&'q CompiledFilter>,
    /// The geo-query (clause 4.10).
    pub geo: Option<&'q CompiledGeoQuery>,
    /// The scope query (clause 4.19).
    pub scope: Option<&'q ScopeQuery>,
}

impl<'q> EntityFilters<'q> {
    /// The filters `selection` carries.
    #[must_use]
    pub const fn of(selection: &'q EntitySelection) -> EntityFilters<'q> {
        EntityFilters {
            q: selection.q.as_ref(),
            geo: selection.geo.as_ref(),
            scope: selection.scope_q.as_ref(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_filters::EntityFilters,
        entity_selection::EntitySelection,
        filter::{Filter, compiled_filter::CompiledFilter, language_matching::LanguageMatching},
        name_stage::Written,
        scope::scope_query::ScopeQuery,
    };

    #[test]
    fn a_selection_carrying_no_filter_gives_no_filter() {
        let selection = EntitySelection::default();
        let filters = EntityFilters::of(&selection);
        assert!(filters.q.is_none() && filters.geo.is_none() && filters.scope.is_none());
    }

    #[test]
    fn the_filters_of_a_selection_are_the_ones_it_carries() {
        let selection = EntitySelection::builder()
            .q(Some(CompiledFilter::new(
                "\"https://example.com/n\"==10"
                    .parse::<Filter<Written>>()
                    .expect("q parses")
                    .into_expanded()
                    .expect("absolute names"),
                LanguageMatching::TagEquality,
            )))
            .scope_q(Some("/A/#".parse::<ScopeQuery>().expect("scopeQ parses")))
            .build();
        let filters = EntityFilters::of(&selection);
        pretty_assertions::assert_eq!(filters.q, selection.q.as_ref());
        pretty_assertions::assert_eq!(filters.scope, selection.scope_q.as_ref());
        assert!(filters.geo.is_none());
    }
}
