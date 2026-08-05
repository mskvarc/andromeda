//! SQL rendering of a `scopeQ` expression (clause 4.18).
//!
//! One walk of the grammar for both backends: entity queries test the current
//! scope paths stored on `entities`, temporal queries test `scope_history`
//! bounded by the query's time window, and the boolean structure between the
//! two is identical.

use crate::{
    error::QueryError,
    scope::{ltree::ltree_matches, pattern::ScopePattern, query::ScopeQuery},
};
use andromeda_domain::temporal::TimeRel;
use chrono::{DateTime, Utc};
use sea_query::{Condition, Expr as SeaExpr, SimpleExpr};

/// Which relation a scope predicate is evaluated against.
#[derive(Debug, Clone, Copy)]
pub enum ScopeTarget<'a> {
    /// The `ltree[]` column carrying an entity's current scope paths.
    CurrentPaths {
        /// Unqualified column name.
        column: &'a str,
    },
    /// `scope_history`, narrowed to the query's time window.
    ///
    /// A temporal query asks which entities were *in* a scope during the
    /// window, not which are in it now, so a row that has since left the scope
    /// still matches (clause 4.18).
    History {
        /// SQL reference to the entity id the probe correlates against.
        entity_id_column: &'a str,
        /// The temporal window, absent when the query bounded nothing.
        window: Option<TimeWindow<'a>>,
    },
}

/// The time window a scope-history probe is bounded by.
#[derive(Debug, Clone, Copy)]
pub struct TimeWindow<'a> {
    /// How `time_at` bounds the window.
    pub timerel: &'a TimeRel,
    /// The reference instant.
    pub time_at: &'a DateTime<Utc>,
    /// The closing instant, required only by `between`.
    pub end_time_at: Option<&'a DateTime<Utc>>,
}

impl<'a> TimeWindow<'a> {
    /// Build a window from the three parts a compiled query carries, or `None`
    /// when the query bounded nothing.
    ///
    /// `timerel` and `timeAt` are meaningless apart (clause 4.5.14), so a
    /// half-specified pair yields no window rather than a partial one.
    pub const fn from_parts(
        timerel: Option<&'a TimeRel>,
        time_at: Option<&'a DateTime<Utc>>,
        end_time_at: Option<&'a DateTime<Utc>>,
    ) -> Option<TimeWindow<'a>> {
        match (timerel, time_at) {
            (Some(timerel), Some(time_at)) => Some(TimeWindow { timerel, time_at, end_time_at }),
            (None, _) | (Some(_), None) => None,
        }
    }
}

/// Render a scope query against the given target.
///
/// The result is a disjunction of conjunctions, mirroring the grammar. A
/// one-alternative, one-requirement query renders as a single predicate:
/// sea-query's `Condition::any` of one term adds no nesting of its own, which
/// is why the hand-written special case both renderers used to carry is gone.
pub fn condition(scope_q: &ScopeQuery, target: ScopeTarget<'_>) -> Result<Condition, QueryError> {
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
fn pattern_predicate(pattern: &ScopePattern, target: ScopeTarget<'_>) -> SimpleExpr {
    let lquery = pattern.to_lquery();
    match target {
        ScopeTarget::CurrentPaths { column } => ltree_matches(SeaExpr::cust(format!("\"{column}\"")), SeaExpr::val(lquery).into()),
        ScopeTarget::History { entity_id_column, window } => scope_history_exists(&lquery, entity_id_column, window),
    }
}

/// Generate an `EXISTS` subquery for a single scope pattern against
/// `scope_history`.
fn scope_history_exists(lquery: &str, entity_id_column: &str, window: Option<TimeWindow<'_>>) -> SimpleExpr {
    // The placeholders are local to this fragment; sea-query renumbers them
    // when the enclosing statement is rendered.
    let mut bind_values: Vec<String> = vec![lquery.to_string()];
    let mut time_conditions = Vec::new();

    if let Some(window) = window {
        let bind = |value: String, values: &mut Vec<String>| {
            values.push(value);
            format!("${}", values.len())
        };
        match window.timerel {
            TimeRel::Before => {
                let marker = bind(window.time_at.to_rfc3339(), &mut bind_values);
                time_conditions.push(format!("\"sh\".\"time\" < {marker}::timestamptz"));
            }
            TimeRel::After => {
                let marker = bind(window.time_at.to_rfc3339(), &mut bind_values);
                time_conditions.push(format!("\"sh\".\"time\" >= {marker}::timestamptz"));
            }
            TimeRel::Between => {
                let start = bind(window.time_at.to_rfc3339(), &mut bind_values);
                time_conditions.push(format!("\"sh\".\"time\" >= {start}::timestamptz"));
                if let Some(end_time) = window.end_time_at {
                    let end = bind(end_time.to_rfc3339(), &mut bind_values);
                    time_conditions.push(format!("\"sh\".\"time\" < {end}::timestamptz"));
                }
            }
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
    use crate::sql::schema::Entities;
    use sea_query::{PostgresQueryBuilder, Query};

    const TEA_ENTITY_ID: &str = "\"tea\".\"entity_id\"";

    fn scope(raw: &str) -> ScopeQuery {
        ScopeQuery::parse(raw).expect("valid scope query")
    }

    fn current(column: &str) -> ScopeTarget<'_> {
        ScopeTarget::CurrentPaths { column }
    }

    fn history(window: Option<TimeWindow<'_>>) -> ScopeTarget<'_> {
        ScopeTarget::History {
            entity_id_column: TEA_ENTITY_ID,
            window,
        }
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
        let sql = render(condition(&scope("/A/B"), current("scope_paths")).unwrap());
        assert!(sql.contains("::lquery"), "{sql}");
        assert!(!sql.contains(" OR "), "{sql}");
        assert_eq!(sql.matches("::lquery").count(), 1, "{sql}");
    }

    #[test]
    fn a_single_scope_history_term_renders_without_nesting() {
        let sql = render(condition(&scope("/A/B"), history(None)).unwrap());
        assert_eq!(sql.matches("EXISTS").count(), 1, "{sql}");
        assert!(!sql.contains(" OR "), "{sql}");
    }

    #[test]
    fn comma_separated_scopes_render_a_disjunction() {
        let sql = render(condition(&scope("/A,/B"), current("scope_paths")).unwrap());
        assert!(sql.contains(" OR "), "{sql}");
    }

    #[test]
    fn semicolon_separated_scopes_render_a_conjunction() {
        let sql = render(condition(&scope("/A;/B"), current("scope_paths")).unwrap());
        assert!(sql.contains(" AND "), "{sql}");
    }

    #[test]
    fn scope_history_renders_one_exists_per_term() {
        let sql = render(condition(&scope("/A;/B,/C"), history(None)).unwrap());
        assert_eq!(sql.matches("EXISTS").count(), 3, "{sql}");
        assert!(sql.contains("scope_history"), "{sql}");
    }

    #[test]
    fn scope_history_bounds_the_probe_by_the_time_window() {
        let time_at = DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z").unwrap().with_timezone(&Utc);
        let end_time_at = DateTime::parse_from_rfc3339("2024-06-01T00:00:00Z").unwrap().with_timezone(&Utc);
        let window = TimeWindow::from_parts(Some(&TimeRel::Between), Some(&time_at), Some(&end_time_at));
        let sql = render(condition(&scope("/A"), history(window)).unwrap());
        assert!(sql.contains("\"sh\".\"time\" >="), "{sql}");
        assert!(sql.contains("\"sh\".\"time\" <"), "{sql}");
    }

    /// Both targets walk the same grammar, so a change to the precedence of
    /// `,` and `;` cannot reach one backend and miss the other.
    #[test]
    fn both_targets_render_the_same_boolean_structure() {
        let scope_q = scope("/A;/B,/C");
        let current_sql = render(condition(&scope_q, current("scope_paths")).unwrap());
        let history_sql = render(condition(&scope_q, history(None)).unwrap());
        for sql in [&current_sql, &history_sql] {
            assert!(sql.contains(" AND "), "{sql}");
            assert!(sql.contains(" OR "), "{sql}");
        }
        assert_eq!(current_sql.matches("::lquery").count(), 3, "{current_sql}");
        assert_eq!(history_sql.matches("::lquery").count(), 3, "{history_sql}");
    }

    #[test]
    fn a_window_needs_both_a_relation_and_an_instant() {
        let time_at = DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z").unwrap().with_timezone(&Utc);
        assert!(TimeWindow::from_parts(None, Some(&time_at), None).is_none());
        assert!(TimeWindow::from_parts(Some(&TimeRel::After), None, None).is_none());
        assert!(TimeWindow::from_parts(Some(&TimeRel::After), Some(&time_at), None).is_some());
    }
}
