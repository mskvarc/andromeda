//! Whether an entity held in memory satisfies a scope query (clause 4.19).

use crate::queried_entity::scoped_entity::ScopedEntity;
use andromeda_ngsi_ld_query::scope::query::ScopeQuery;

/// Whether the scopes `entity` belongs to (clause 4.18) satisfy `query`.
pub fn scope_query_matches<'a, E: ScopedEntity<'a>>(query: &ScopeQuery, entity: E) -> bool {
    query.matches(entity.scopes())
}

#[cfg(test)]
mod tests {
    use crate::{queried_entity::scoped_entity::ScopedEntity, scope_query::scope_query_matches};
    use andromeda_ngsi_ld_lexicon::scope::path::ScopePath;
    use andromeda_ngsi_ld_query::scope::query::ScopeQuery;

    /// An entity carrying the given scopes (clause 4.18).
    struct ScopedFixture(Vec<ScopePath>);

    impl<'a> ScopedEntity<'a> for &'a ScopedFixture {
        fn scopes(self) -> impl Iterator<Item = &'a ScopePath> + Clone {
            self.0.iter()
        }
    }

    fn scoped(paths: &[&str]) -> ScopedFixture {
        ScopedFixture(paths.iter().map(|path| path.parse().expect("a scope path")).collect())
    }

    /// Clause 4.19 read against an entity's scopes: every requirement of one
    /// alternative met by some scope, and no scope at all meeting nothing.
    #[test]
    fn an_entity_is_matched_by_the_scopes_it_carries() {
        let query = ScopeQuery::parse("(/Madrid/#;/Spain)").expect("parses");
        assert!(scope_query_matches(&query, &scoped(&["/Spain", "/Madrid/Gardens"])));
        assert!(!scope_query_matches(&query, &scoped(&["/Spain"])));
        assert!(!scope_query_matches(&query, &scoped(&[])));
    }
}
