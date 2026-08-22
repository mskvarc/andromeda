//! A `scopeQ` (clause 4.19) rendered against a registration's `scope`.
//!
//! A registration stores its scopes as the clause 4.18 paths it was given, in
//! a `text[]`, so each pattern is tested by unnesting the array and matching
//! every path against the anchored regular expression the pattern denotes. The
//! reading is the one the in-memory `ScopePattern::matches` gives: a label
//! matches itself, `+` one level of any name, and a `#` the anchor itself or
//! any descendant of it (a bare `/#` any non-empty scope).

use andromeda_ngsi_ld_query::scope::{
    pattern::{ScopePattern, ScopeSegment},
    query::ScopeQuery,
};
use andromeda_postgresql_schema::csr::Csr;
use andromeda_postgresql_seaquery::{literal::int_literal, operator::regex_match, unnest::unnest_of};
use sea_query::{Alias, Condition, Expr as SeaExpr, Query, SimpleExpr};

/// The disjunction of conjunctions a scope query is, each pattern tested
/// against the registration's scopes. A registration declaring no scope has
/// nothing a pattern could match and is not selected.
pub(crate) fn registration_scope_matches(scope_q: &ScopeQuery) -> Condition {
    let mut disjunction = Condition::any();
    for alternative in scope_q.alternatives() {
        let mut conjunction = Condition::all();
        for pattern in alternative {
            conjunction = conjunction.add(some_scope_matches(pattern));
        }
        disjunction = disjunction.add(conjunction);
    }
    disjunction
}

/// `EXISTS (SELECT 1 FROM unnest("scope") AS s WHERE "s"."s" ~ <pattern regex>)`.
fn some_scope_matches(pattern: &ScopePattern) -> SimpleExpr {
    let mut scan = Query::select();
    scan.expr(int_literal(1))
        .from_function(unnest_of(SeaExpr::col(Csr::Scope)), Alias::new("s"))
        .and_where(regex_match(
            SeaExpr::col((Alias::new("s"), Alias::new("s"))),
            SeaExpr::val(pattern_regex(pattern)),
        ));
    SeaExpr::exists(scan)
}

/// The anchored POSIX regular expression a clause 4.19 pattern denotes over a
/// stored scope path. Labels need no escaping: the clause 4.18 grammar admits
/// letters, digits and underscores only.
fn pattern_regex(pattern: &ScopePattern) -> String {
    let mut regex = String::from("^/?");
    for (index, segment) in pattern.segments().iter().enumerate() {
        match segment {
            ScopeSegment::Label(label) => {
                if index > 0 {
                    regex.push('/');
                }
                regex.push_str(label.as_str());
            }
            ScopeSegment::SingleLevel => {
                if index > 0 {
                    regex.push('/');
                }
                regex.push_str("[^/]+");
            }
            // A prefix `#` admits the anchor or any descendant; a bare `/#`
            // any non-empty scope.
            ScopeSegment::MultiLevel if index == 0 => regex.push_str("[^/]+(/[^/]+)*"),
            ScopeSegment::MultiLevel => regex.push_str("(/[^/]+)*"),
        }
    }
    regex.push('$');
    regex
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_query::PostgresQueryBuilder;

    fn render(raw: &str) -> String {
        let scope_q = ScopeQuery::parse(raw).expect("valid scope query");
        Query::select()
            .expr(SeaExpr::cust("1"))
            .from(Alias::new("context_source_registrations"))
            .cond_where(registration_scope_matches(&scope_q))
            .to_string(PostgresQueryBuilder)
    }

    fn regex(raw: &str) -> String {
        pattern_regex(&ScopePattern::parse(raw).expect("valid pattern"))
    }

    #[test]
    fn a_pattern_is_tested_against_every_stored_scope() {
        let sql = render("/Madrid/#");
        assert!(sql.contains("UNNEST(\"scope\") AS \"s\""), "{sql}");
        assert!(sql.contains("\"s\".\"s\" ~ '^/?Madrid(/[^/]+)*$'"), "{sql}");
    }

    /// The regex reads the pattern as `ScopePattern::matches` does.
    #[test]
    fn the_pattern_regex_mirrors_the_in_memory_reading() {
        assert_eq!(regex("/A/B"), "^/?A/B$");
        assert_eq!(regex("/A/+"), "^/?A/[^/]+$");
        assert_eq!(regex("/A/#"), "^/?A(/[^/]+)*$");
        assert_eq!(regex("/#"), "^/?[^/]+(/[^/]+)*$");
    }

    #[test]
    fn alternatives_disjoin_and_requirements_conjoin() {
        let sql = render("/A;/B,/C");
        assert_eq!(sql.matches("EXISTS").count(), 3, "{sql}");
        assert!(sql.contains(" AND "), "{sql}");
        assert!(sql.contains(" OR "), "{sql}");
    }
}
