//! SQL rendering of a `scopeQ` expression (clause 4.18).
//!
//! One walk of the grammar for both backends: entity queries test the current
//! scope paths stored on `entities`, temporal queries test `scope_history`
//! bounded by the query's time window, and the boolean structure between the
//! two is identical.

use crate::{
    bind::BindBuilder,
    column_reference::ColumnReference,
    ltree::{ltree_matches, pattern_to_lquery},
    time_column::column_name,
};
use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
use andromeda_ngsi_ld_query::{
    scope::{pattern::ScopePattern, query::ScopeQuery},
    temporal::window::TimeInterval,
};
use sea_query::{Condition, Expr as SeaExpr, IntoIden, SimpleExpr};

/// Which relation a scope predicate is evaluated against.
#[derive(Debug, Clone)]
pub enum ScopeTarget {
    /// The `ltree[]` column carrying an entity's current scope paths.
    CurrentPaths {
        /// The column carrying an entity's current scope paths, named by its iden.
        column: ColumnReference,
    },
    /// `scope_history`, narrowed to the query's time window.
    ///
    /// A temporal query asks which entities were *in* a scope during the
    /// window, not which are in it now, so a row that has since left the scope
    /// still matches (clause 4.18).
    ///
    /// Scope history hangs off the temporal catalogue rather than off current
    /// entity state, so the probe correlates on the surrogate
    /// `temporal_entity_key` an entity keeps after clause 5.6.6 deletion.
    History {
        /// The temporal entity key the probe correlates against, named by its iden.
        entity_key_column: ColumnReference,
        /// The Temporal Property the window is measured on (clause 5.2.21).
        timeproperty: TimeProp,
        /// The slice of that axis the probe is bounded to.
        interval: TimeInterval,
    },
}

impl ScopeTarget {
    /// The current-scope target reading an entity's scope paths from `column`.
    ///
    /// The column is named by the iden the driving statement declares, so a
    /// rename moves this reference with it rather than leaving a hand-quoted
    /// string behind.
    pub fn current_paths(column: impl IntoIden) -> ScopeTarget {
        ScopeTarget::CurrentPaths {
            column: ColumnReference::bare(column),
        }
    }

    /// The scope-history target correlating on the `(alias, column)` temporal
    /// entity key, bounded to `interval` on the `timeproperty` axis.
    pub fn history(entity_key_column: (impl IntoIden, impl IntoIden), timeproperty: TimeProp, interval: TimeInterval) -> ScopeTarget {
        ScopeTarget::History {
            entity_key_column: ColumnReference::qualified(entity_key_column.0, entity_key_column.1),
            timeproperty,
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
pub fn condition(scope_q: &ScopeQuery, target: &ScopeTarget) -> Condition {
    let mut disjunction = Condition::any();
    for alternative in scope_q.alternatives() {
        let mut conjunction = Condition::all();
        for pattern in alternative {
            conjunction = conjunction.add(pattern_predicate(pattern, target));
        }
        disjunction = disjunction.add(conjunction);
    }
    disjunction
}

/// Render one scope pattern against the target.
fn pattern_predicate(pattern: &ScopePattern, target: &ScopeTarget) -> SimpleExpr {
    let lquery = pattern_to_lquery(pattern);
    match target {
        // The reference is rendered from its iden here rather than stored quoted.
        ScopeTarget::CurrentPaths { column } => ltree_matches(SeaExpr::cust(column.render()), SeaExpr::val(lquery).into()),
        ScopeTarget::History {
            entity_key_column,
            timeproperty,
            interval,
        } => scope_history_exists(&lquery, &entity_key_column.render(), *timeproperty, *interval),
    }
}

/// Generate an `EXISTS` subquery for a single scope pattern against
/// `scope_history`.
fn scope_history_exists(lquery: &str, entity_key_column: &str, timeproperty: TimeProp, interval: TimeInterval) -> SimpleExpr {
    // The placeholders are local to this fragment; sea-query renumbers them
    // when the enclosing statement is rendered. `$1` is the lquery pattern the
    // EXISTS body probes with.
    let mut binds: BindBuilder<String> = BindBuilder::default();
    let lquery_marker = binds.bind(lquery.to_string());
    let mut time_conditions = Vec::new();
    // The window is measured on the requested Temporal Property alone, exactly
    // as an attribute-history window is (clauses 4.11 and 5.2.21): a scope
    // observation carrying no instant on that axis takes no position on it.
    let axis = column_name(timeproperty);

    match interval {
        TimeInterval::Unbounded => {}
        TimeInterval::Before(instant) => {
            let marker = binds.bind(instant.to_rfc3339());
            time_conditions.push(format!("\"sh\".\"{axis}\" < {marker}::timestamptz"));
        }
        TimeInterval::After(instant) => {
            let marker = binds.bind(instant.to_rfc3339());
            time_conditions.push(format!("\"sh\".\"{axis}\" >= {marker}::timestamptz"));
        }
        TimeInterval::Between { from, to } => {
            let start = binds.bind(from.to_rfc3339());
            time_conditions.push(format!("\"sh\".\"{axis}\" >= {start}::timestamptz"));
            let end = binds.bind(to.to_rfc3339());
            time_conditions.push(format!("\"sh\".\"{axis}\" < {end}::timestamptz"));
        }
    }

    let time_clause = if time_conditions.is_empty() {
        String::new()
    } else {
        format!(" AND {}", time_conditions.join(" AND "))
    };

    SeaExpr::cust_with_values(
        format!(
            "EXISTS (SELECT 1 FROM \"scope_history\" AS \"sh\" WHERE \"sh\".\"temporal_entity_key\" = {entity_key_column} AND \"sh\".\"scope_path\" ~ {lquery_marker}::lquery{time_clause})"
        ),
        binds.into_values(),
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
        ScopeTarget::history((Alias::new("e"), Alias::new("entity_key")), TimeProp::Observed, interval)
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
        let sql = render(condition(&scope("/A/B"), &current()));
        assert!(sql.contains("::lquery"), "{sql}");
        assert!(!sql.contains(" OR "), "{sql}");
        assert_eq!(sql.matches("::lquery").count(), 1, "{sql}");
    }

    #[test]
    fn a_single_scope_history_term_renders_without_nesting() {
        let sql = render(condition(&scope("/A/B"), &history(TimeInterval::Unbounded)));
        assert_eq!(sql.matches("EXISTS").count(), 1, "{sql}");
        assert!(!sql.contains(" OR "), "{sql}");
    }

    #[test]
    fn comma_separated_scopes_render_a_disjunction() {
        let sql = render(condition(&scope("/A,/B"), &current()));
        assert!(sql.contains(" OR "), "{sql}");
    }

    #[test]
    fn semicolon_separated_scopes_render_a_conjunction() {
        let sql = render(condition(&scope("/A;/B"), &current()));
        assert!(sql.contains(" AND "), "{sql}");
    }

    #[test]
    fn scope_history_renders_one_exists_per_term() {
        let sql = render(condition(&scope("/A;/B,/C"), &history(TimeInterval::Unbounded)));
        assert_eq!(sql.matches("EXISTS").count(), 3, "{sql}");
        assert!(sql.contains("scope_history"), "{sql}");
    }

    #[test]
    fn scope_history_bounds_the_probe_by_the_time_window() {
        let interval = TimeInterval::Between {
            from: instant("2024-01-01T00:00:00Z"),
            to: instant("2024-06-01T00:00:00Z"),
        };
        let sql = render(condition(&scope("/A"), &history(interval)));
        assert!(sql.contains("\"sh\".\"observed_at\" >="), "{sql}");
        assert!(sql.contains("\"sh\".\"observed_at\" <"), "{sql}");
    }

    /// The rebuilt `scope_history` hangs off the temporal catalogue: it carries
    /// `temporal_entity_key` and the four semantic axes, and no `entity_id` or
    /// `time` column for the probe to reach for.
    #[test]
    fn scope_history_probes_the_columns_the_rebuilt_relation_declares() {
        let sql = render(condition(&scope("/A"), &history(TimeInterval::Unbounded)));
        assert!(sql.contains("\"sh\".\"temporal_entity_key\" = \"e\".\"entity_key\""), "{sql}");
        assert!(!sql.contains("\"sh\".\"entity_id\""), "{sql}");
        assert!(!sql.contains("\"sh\".\"time\""), "{sql}");
    }

    /// Clause 5.2.21: each `timeproperty` bounds the scope probe on its own
    /// column, exactly as it bounds an attribute-history window.
    #[test]
    fn each_time_property_bounds_the_scope_probe_on_its_own_column() {
        let expected = [
            (TimeProp::Observed, "observed_at"),
            (TimeProp::Created, "created_at"),
            (TimeProp::Modified, "modified_at"),
            (TimeProp::Deleted, "deleted_at"),
        ];
        for (timeproperty, column) in expected {
            let target = ScopeTarget::history(
                (Alias::new("e"), Alias::new("entity_key")),
                timeproperty,
                TimeInterval::After(instant("2024-01-01T00:00:00Z")),
            );
            let sql = render(condition(&scope("/A"), &target));
            assert!(sql.contains(&format!("\"sh\".\"{column}\" >=")), "{sql}");
        }
    }

    /// Both targets walk the same grammar, so a change to the precedence of
    /// `,` and `;` cannot reach one backend and miss the other.
    #[test]
    fn both_targets_render_the_same_boolean_structure() {
        let scope_q = scope("/A;/B,/C");
        let current_sql = render(condition(&scope_q, &current()));
        let history_sql = render(condition(&scope_q, &history(TimeInterval::Unbounded)));
        for sql in [&current_sql, &history_sql] {
            assert!(sql.contains(" AND "), "{sql}");
            assert!(sql.contains(" OR "), "{sql}");
        }
        assert_eq!(current_sql.matches("::lquery").count(), 3, "{current_sql}");
        assert_eq!(history_sql.matches("::lquery").count(), 3, "{history_sql}");
    }
}
