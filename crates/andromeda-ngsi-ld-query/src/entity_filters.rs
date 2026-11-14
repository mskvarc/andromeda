//! The value, geo and scope filters that select one entity.
//!
//! The `q` expression (clause 4.9), the geo-query (clause 4.10) and the scope
//! query (clause 4.19) select an entity together: the entities a query
//! answers with match all three it carries (clause 5.7.2.4), and a
//! Subscription notifies of an entity that matches its query, geoquery and
//! scopeQuery (clause 5.8.6). The record borrows the three, compiled, from
//! wherever they are held, so an entity query, a split query and a
//! Subscription hand the same filters to one admission.

use crate::{
    entity_selection::EntitySelection,
    filter::{expression::Expr, language_matching::LanguageMatching},
    geo::query::GeoQuery,
    scope::query::ScopeQuery,
};

/// The value, geo and scope filters an entity is selected by, each absent when
/// the query or Subscription carries none.
#[derive(Debug, Clone, Copy)]
pub struct EntityFilters<'q> {
    /// The `q` expression (clause 4.9), expanded.
    pub q: Option<&'q Expr>,
    /// How the target languages of `q` select languageMap keys.
    pub language_matching: LanguageMatching,
    /// The geo-query (clause 4.10).
    pub geo: Option<&'q GeoQuery>,
    /// The scope query (clause 4.19).
    pub scope: Option<&'q ScopeQuery>,
}

impl<'q> EntityFilters<'q> {
    /// The filters `selection` carries.
    #[must_use]
    pub const fn of(selection: &'q EntitySelection) -> EntityFilters<'q> {
        EntityFilters {
            q: selection.q_expr.as_ref(),
            language_matching: selection.language_matching,
            geo: selection.geo.as_ref(),
            scope: selection.scope_q.as_ref(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{entity_filters::EntityFilters, entity_selection::EntitySelection, filter::parser::parse, scope::query::ScopeQuery};

    #[test]
    fn a_selection_carrying_no_filter_gives_no_filter() {
        let selection = EntitySelection::default();
        let filters = EntityFilters::of(&selection);
        assert!(filters.q.is_none() && filters.geo.is_none() && filters.scope.is_none());
    }

    #[test]
    fn the_filters_of_a_selection_are_the_ones_it_carries() {
        let selection = EntitySelection::builder()
            .q_expr(Some(parse("\"https://example.com/n\"==10").expect("q parses")))
            .scope_q(Some(ScopeQuery::parse("/A/#").expect("scopeQ parses")))
            .build();
        let filters = EntityFilters::of(&selection);
        pretty_assertions::assert_eq!(filters.q, selection.q_expr.as_ref());
        pretty_assertions::assert_eq!(filters.scope, selection.scope_q.as_ref());
        assert!(filters.geo.is_none());
    }
}
