//! SQL rendering of a `scopeQ` expression (clause 4.18).
//!
//! One walk of the grammar for both backends: entity queries test the current
//! scope paths stored on `entities`, temporal queries test `scope_history`
//! bounded by the query's time window, and the boolean structure between the
//! two is identical.

use crate::{
    column_reference::{qualified, quoted},
    error::RenderError,
    ltree::{ltree_matches, pattern_to_lquery},
};
use andromeda_ngsi_ld_query::{
    scope::{pattern::ScopePattern, query::ScopeQuery},
    temporal::window::TimeInterval,
};
use sea_query::{Condition, Expr as SeaExpr, Iden, SimpleExpr};

/// Which relation a scope predicate is evaluated against.
#[derive(Debug, Clone)]
pub enum ScopeTarget {
    /// The `ltree[]` column carrying an entity's current scope paths.
    CurrentPaths {
        /// Quoted SQL reference to the column, derived from its iden.
        column: String,
    },
    /// `scope_history`, narrowed to the query's time window.
    ///
    /// A temporal query asks which entities were *in* a scope during the
    /// window, not which are in it now, so a row that has since left the scope
    /// still matches (clause 4.18).
    History {
        /// Quoted SQL reference to the entity id the probe correlates against.
        entity_id_column: String,
        /// The slice of the time axis the probe is bounded to.
        interval: TimeInterval,
    },
}

impl ScopeTarget {
    /// The current-scope target reading an entity's scope paths from `column`.
    ///
    /// The column is named by the iden the driving statement declares, so a
    /// rename moves this reference with it rather than leaving a hand-quoted
    /// string behind.
    pub fn current_paths(column: impl Iden) -> ScopeTarget {
        ScopeTarget::CurrentPaths { column: quoted(column) }
    }

    /// The scope-history target correlating on the `(alias, column)` entity id,
    /// bounded to `interval`.
    pub fn history(entity_id_column: (impl Iden, impl Iden), interval: TimeInterval) -> ScopeTarget {
        ScopeTarget::History {
            entity_id_column: qualified(entity_id_column.0, entity_id_column.1),
            interval,
        }
    }
}

/// Render a scope query against the given target.
///
/// The result is a disjunction of conjunctions, mirroring the grammar. A
/// one-alternative, one-requirement query renders as a single predicate:
/// sea-query's `Condition::any` of one term adds no nesting of its own, which
/// is why the hand-written special case both renderers used to carry is gone.
pub fn condition(scope_q: &ScopeQuery, target: &ScopeTarget) -> Result<Condition, RenderError> {
    let mut disjunction = Condition::any();
    for alternative in scope_q.alternatives() {
        let mut conjunction = Condition::all();
        for pattern in alternative {
            conjunction = conjunction.add(pattern_predicate(pattern, target));
        }
        disjunction = disjunction.add(conjunction);
    }
    Ok(disjunction)
}

/// Render one scope pattern against the target.
fn pattern_predicate(pattern: &ScopePattern, target: &ScopeTarget) -> SimpleExpr {
    let lquery = pattern_to_lquery(pattern);
    match target {
        // The column reference is already quoted by the constructor.
        ScopeTarget::CurrentPaths { column } => ltree_matches(SeaExpr::cust(column.as_str()), SeaExpr::val(lquery).into()),
        ScopeTarget::History { entity_id_column, interval } => scope_history_exists(&lquery, entity_id_column, *interval),
    }
}

/// Generate an `EXISTS` subquery for a single scope pattern against
/// `scope_history`.
fn scope_history_exists(lquery: &str, entity_id_column: &str, interval: TimeInterval) -> SimpleExpr {
    // The placeholders are local to this fragment; sea-query renumbers them
    // when the enclosing statement is rendered.
    let mut bind_values: Vec<String> = vec![lquery.to_string()];
    let mut time_conditions = Vec::new();

    let bind = |value: String, values: &mut Vec<String>| {
        values.push(value);
        format!("${}", values.len())
    };
    match interval {
        TimeInterval::Unbounded => {}
        TimeInterval::Before(instant) => {
            let marker = bind(instant.to_rfc3339(), &mut bind_values);
            time_conditions.push(format!("\"sh\".\"time\" < {marker}::timestamptz"));
        }
        TimeInterval::After(instant) => {
            let marker = bind(instant.to_rfc3339(), &mut bind_values);
            time_conditions.push(format!("\"sh\".\"time\" >= {marker}::timestamptz"));
        }
        TimeInterval::Between { from, to } => {
            let start = bind(from.to_rfc3339(), &mut bind_values);
            time_conditions.push(format!("\"sh\".\"time\" >= {start}::timestamptz"));
            let end = bind(to.to_rfc3339(), &mut bind_values);
            time_conditions.push(format!("\"sh\".\"time\" < {end}::timestamptz"));
        }
    }

    let time_clause = if time_conditions.is_empty() {
        String::new()
    } else {
        format!(" AND {}", time_conditions.join(" AND "))
    };

    SeaExpr::cust_with_values(
        format!(
            "EXISTS (SELECT 1 FROM \"scope_history\" AS \"sh\" WHERE \"sh\".\"entity_id\" = {entity_id_column} AND \"sh\".\"scope_path\" ~ $1::lquery{time_clause})"
        ),
        bind_values,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Entities;
    use sea_query::{Alias, PostgresQueryBuilder, Query};

    fn scope(raw: &str) -> ScopeQuery {
        ScopeQuery::parse(raw).expect("valid scope query")
    }

    fn current() -> ScopeTarget {
        ScopeTarget::current_paths(Entities::ScopePaths)
    }

    fn history(interval: TimeInterval) -> ScopeTarget {
        ScopeTarget::history((Alias::new("tea"), Alias::new("entity_id")), interval)
    }

    fn instant(raw: &str) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339(raw).expect("valid instant").with_timezone(&chrono::Utc)
    }

    fn render(condition: Condition) -> String {
        Query::select()
            .column(Entities::Id)
            .from(Entities::Table)
            .cond_where(condition)
            .to_string(PostgresQueryBuilder)
    }

    /// A one-term disjunction renders without nesting, so the hand-written
    /// "if only one OR term, return it directly" special case both renderers
    /// carried was redundant.
    #[test]
    fn a_single_scope_term_renders_without_nesting() {
        let sql = render(condition(&scope("/A/B"), &current()).unwrap());
        assert!(sql.contains("::lquery"), "{sql}");
        assert!(!sql.contains(" OR "), "{sql}");
        assert_eq!(sql.matches("::lquery").count(), 1, "{sql}");
    }

    #[test]
    fn a_single_scope_history_term_renders_without_nesting() {
        let sql = render(condition(&scope("/A/B"), &history(TimeInterval::Unbounded)).unwrap());
        assert_eq!(sql.matches("EXISTS").count(), 1, "{sql}");
        assert!(!sql.contains(" OR "), "{sql}");
    }

    #[test]
    fn comma_separated_scopes_render_a_disjunction() {
        let sql = render(condition(&scope("/A,/B"), &current()).unwrap());
        assert!(sql.contains(" OR "), "{sql}");
    }

    #[test]
    fn semicolon_separated_scopes_render_a_conjunction() {
        let sql = render(condition(&scope("/A;/B"), &current()).unwrap());
        assert!(sql.contains(" AND "), "{sql}");
    }

    #[test]
    fn scope_history_renders_one_exists_per_term() {
        let sql = render(condition(&scope("/A;/B,/C"), &history(TimeInterval::Unbounded)).unwrap());
        assert_eq!(sql.matches("EXISTS").count(), 3, "{sql}");
        assert!(sql.contains("scope_history"), "{sql}");
    }

    #[test]
    fn scope_history_bounds_the_probe_by_the_time_window() {
        let interval = TimeInterval::Between {
            from: instant("2024-01-01T00:00:00Z"),
            to: instant("2024-06-01T00:00:00Z"),
        };
        let sql = render(condition(&scope("/A"), &history(interval)).unwrap());
        assert!(sql.contains("\"sh\".\"time\" >="), "{sql}");
        assert!(sql.contains("\"sh\".\"time\" <"), "{sql}");
    }

    /// Both targets walk the same grammar, so a change to the precedence of
    /// `,` and `;` cannot reach one backend and miss the other.
    #[test]
    fn both_targets_render_the_same_boolean_structure() {
        let scope_q = scope("/A;/B,/C");
        let current_sql = render(condition(&scope_q, &current()).unwrap());
        let history_sql = render(condition(&scope_q, &history(TimeInterval::Unbounded)).unwrap());
        for sql in [&current_sql, &history_sql] {
            assert!(sql.contains(" AND "), "{sql}");
            assert!(sql.contains(" OR "), "{sql}");
        }
        assert_eq!(current_sql.matches("::lquery").count(), 3, "{current_sql}");
        assert_eq!(history_sql.matches("::lquery").count(), 3, "{history_sql}");
    }
}
