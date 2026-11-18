//! Whether an entity held in memory satisfies a scope query (clause 4.19).

use andromeda_ngsi_ld_lexicon::scope::path::ScopePath;
use andromeda_ngsi_ld_query::scope::scope_query::ScopeQuery;

/// Whether `scopes` satisfy `query`.
///
/// Clause 4.19: an alternative holds when every one of its requirements is
/// satisfied by at least one of the scopes, and the query holds when any
/// alternative does. The scopes are walked once per requirement, so their
/// iterator has to be cheap to clone. An entity passes the scopes it belongs
/// to (clause 4.18), a registration the `scope` it declares.
pub fn scope_query_matches<'a, I>(query: &ScopeQuery, scopes: I) -> bool
where
    I: IntoIterator<Item = &'a ScopePath>,
    I::IntoIter: Clone,
{
    let scopes = scopes.into_iter();
    query
        .alternatives()
        .any(|mut alternative| alternative.all(|pattern| scopes.clone().any(|scope| pattern.matches(scope))))
}

#[cfg(test)]
mod tests {
    use crate::{queried_entity::scoped_entity::ScopedEntity, scope_query::scope_query_matches};
    use andromeda_ngsi_ld_lexicon::scope::path::ScopePath;
    use andromeda_ngsi_ld_query::scope::scope_query::ScopeQuery;

    /// An entity carrying the given scopes (clause 4.18).
    struct ScopedFixture(Vec<ScopePath>);

    impl<'a> ScopedEntity<'a> for &'a ScopedFixture {
        fn scopes(self) -> impl Iterator<Item = &'a ScopePath> + Clone {
            self.0.iter()
        }
    }

    fn paths(raw: &[&str]) -> Vec<ScopePath> {
        raw.iter().map(|path| path.parse().expect("a scope path")).collect()
    }

    fn scoped(raw: &[&str]) -> ScopedFixture {
        ScopedFixture(paths(raw))
    }

    /// Clause 4.19 read against an entity's scopes: every requirement of one
    /// alternative met by some scope, and no scope at all meeting nothing.
    #[test]
    fn an_entity_is_matched_by_the_scopes_it_carries() {
        let query = "(/Madrid/#;/Spain)".parse::<ScopeQuery>().expect("parses");
        assert!(scope_query_matches(&query, (&scoped(&["/Spain", "/Madrid/Gardens"])).scopes()));
        assert!(!scope_query_matches(&query, (&scoped(&["/Spain"])).scopes()));
        assert!(!scope_query_matches(&query, (&scoped(&[])).scopes()));
    }

    #[test]
    fn a_conjunction_needs_every_requirement_met_by_some_scope() {
        let query = "(/Spain;/Madrid/Gardens)".parse::<ScopeQuery>().expect("parses");
        assert!(scope_query_matches(&query, &paths(&["/Spain", "/Madrid/Gardens"])));
        assert!(!scope_query_matches(&query, &paths(&["/Spain"])));
    }

    #[test]
    fn a_disjunction_needs_only_one_alternative_met() {
        let query = "/France,/Spain".parse::<ScopeQuery>().expect("parses");
        assert!(scope_query_matches(&query, &paths(&["/Spain"])));
        assert!(scope_query_matches(&query, &paths(&["/France"])));
        assert!(!scope_query_matches(&query, &paths(&["/Italy"])));
    }

    /// Clause 4.19 EXAMPLE 5: `(/Madrid/Districts;/CompanyA)|/CompanyB`.
    #[test]
    fn a_grouped_conjunction_or_a_pattern_matches_either() {
        let query = "(/A;/B)|/C".parse::<ScopeQuery>().expect("parses");
        assert!(scope_query_matches(&query, &paths(&["/A", "/B"])));
        assert!(scope_query_matches(&query, &paths(&["/C"])));
        assert!(!scope_query_matches(&query, &paths(&["/A"])));
    }

    /// Clause 4.19 spells rooted patterns only; a clause 4.18 Scope written
    /// without its leading `/` meets them by its levels.
    #[test]
    fn a_scope_query_matches_bare_and_rooted_scopes_alike() {
        let query = "(/A;/B)".parse::<ScopeQuery>().expect("parses");
        assert!(scope_query_matches(&query, &paths(&["A", "/B"])));
    }
}
