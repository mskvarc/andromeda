//! SQL rendering of a `scopeQ` expression (clause 4.19).
//!
//! One walk of the grammar for two targets: an array of scope paths (an
//! entity's or a registration's current scopes, or the scope one state of a
//! Temporal Evolution holds), and the clause 4.18 validity intervals
//! reconstructed from `scope_history`, asked whether one overlaps a temporal
//! query's window. The boolean structure between them is identical.
//!
//! The interval reading itself lives in [`crate::scope_validity`], which the
//! reified scope of clause 4.5.6 reads through as well, so the two cannot
//! disagree about when a stored scope event stopped applying.

use crate::{
    history_source::{HistoryBranch, HistoryOf, scope_rows::scope_rows},
    ltree::pattern_to_lquery,
    scope_validity::{ScopeEvent, overlaps_window, positioned_on_axis, scope_history_axis},
};
use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
use andromeda_ngsi_ld_query::{
    scope::{pattern::ScopePattern, query::ScopeQuery},
    temporal::window::TimeInterval,
};
use andromeda_postgresql_schema::temporal::ScopeHistory;
use andromeda_postgresql_seaquery::{column_reference::ColumnReference, ltree::ltree_matches};
use sea_query::{Condition, Expr as SeaExpr, IntoIden, Query, SelectStatement, SimpleExpr};

/// Which relation a scope predicate is evaluated against.
#[derive(Debug, Clone)]
pub enum ScopeTarget {
    /// An `ltree[]` column carrying the scope paths an Entity holds: its
    /// current ones, a registration's, or those in force over one state of its
    /// Temporal Evolution.
    Paths {
        /// The column carrying the scope paths, named by its iden.
        column: ColumnReference,
    },
    /// `scope_history`, read as the clause 4.18 validity intervals a scope has
    /// and asked whether any of them overlaps the query's window.
    ///
    /// A temporal query asks which entities were *in* a scope during the
    /// window, not which are in it now, so a scope set before the window and
    /// never removed matches, and a scope that had already been superseded when
    /// the window opened does not.
    ///
    /// Scope history hangs off the temporal catalogue rather than off current
    /// entity state, so the probe correlates on the Entity identifier its
    /// Temporal Evolution keeps after clause 5.6.6 deletion.
    History {
        /// The Entity identifier the probe correlates against, named by its iden.
        entity_id_column: ColumnReference,
        /// The Temporal Property the window is measured on (clause 5.2.21).
        timeproperty: TimeProp,
        /// The window a validity interval has to overlap.
        interval: TimeInterval,
        /// The branch whose scope history the probe reads.
        branch: HistoryBranch,
    },
}

impl ScopeTarget {
    /// The target reading the scope paths from the `(relation, column)` the
    /// driving statement declares.
    ///
    /// The column is named by the idens the driving statement declares, so a
    /// rename moves this reference with it rather than leaving a hand-quoted
    /// string behind; it is qualified because the statement joins the search
    /// row beside the identity row and an unqualified name would be ambiguous.
    pub fn paths(column: (impl IntoIden, impl IntoIden)) -> ScopeTarget {
        ScopeTarget::Paths {
            column: ColumnReference::qualified(column.0, column.1),
        }
    }

    /// The scope-history target correlating on the `(alias, column)` Entity
    /// identifier, asking whether a validity interval overlaps `interval` on
    /// the `timeproperty` axis.
    pub fn history(entity_id_column: (impl IntoIden, impl IntoIden), timeproperty: TimeProp, interval: TimeInterval, branch: HistoryBranch) -> ScopeTarget {
        ScopeTarget::History {
            entity_id_column: ColumnReference::qualified(entity_id_column.0, entity_id_column.1),
            timeproperty,
            interval,
            branch,
        }
    }
}

/// Render a scope query against the given target.
///
/// The result is a disjunction of conjunctions, mirroring the grammar. A
/// one-alternative, one-requirement query renders as a single predicate:
/// sea-query's `Condition::any` of one term adds no nesting of its own.
#[must_use]
pub fn condition(scope_q: &ScopeQuery, target: &ScopeTarget) -> Condition {
    let mut disjunction = Condition::any();
    for alternative in scope_q.alternatives() {
        let mut conjunction = Condition::all();
        for pattern in alternative {
            conjunction = conjunction.add(pattern_condition(pattern, target));
        }
        disjunction = disjunction.add(conjunction);
    }
    disjunction
}

/// Render one scope pattern -- one requirement of one alternative (clause
/// 4.19) -- against the target.
///
/// A split temporal query reads every requirement on its own (clause 5.7.4.4),
/// so the requirement is a rendering of its own as well as a term of
/// [`condition`].
#[must_use]
pub fn pattern_condition(pattern: &ScopePattern, target: &ScopeTarget) -> SimpleExpr {
    let lquery = pattern_to_lquery(pattern);
    match target {
        ScopeTarget::Paths { column } => ltree_matches(column.column_expr(), SeaExpr::val(lquery)),
        ScopeTarget::History {
            entity_id_column,
            timeproperty,
            interval,
            branch,
        } => {
            let axis = scope_history_axis(*timeproperty);
            SeaExpr::exists(matching_scope_events(
                &lquery,
                entity_id_column,
                axis,
                overlaps_window(axis, *interval, branch),
                branch,
            ))
        }
    }
}

/// The correlated probe over the scope events of one entity that name a path
/// matching `lquery`, narrowed further by the validity condition the target
/// asks for.
///
/// A removal carries no path — `chk_scope_history_value_presence` forbids one —
/// so it never satisfies the pattern match and only ever appears as the event
/// that ends someone else's interval. That is also what makes a `scopeQ`
/// measured on the `deletedAt` axis select nothing at all: `deletedAt` is
/// carried by removals alone, and a removal names no scope to match.
fn matching_scope_events(lquery: &str, entity_id_column: &ColumnReference, axis: ScopeHistory, validity: Condition, branch: &HistoryBranch) -> SelectStatement {
    let mut probe = Query::select();
    probe
        .expr(SeaExpr::val(1))
        .from(scope_rows(branch, &HistoryOf::Entity(entity_id_column.column_expr()), ScopeEvent))
        .and_where(positioned_on_axis(axis))
        .and_where(ltree_matches(
            SeaExpr::col((ScopeEvent, ScopeHistory::ScopePath)),
            SeaExpr::val(lquery.to_string()),
        ))
        .cond_where(validity);
    probe.take()
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_postgresql_schema::current::{Entities, EntitySearch};
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, PostgresQueryBuilder, Query};

    fn scope(raw: &str) -> ScopeQuery {
        ScopeQuery::parse(raw).expect("valid scope query")
    }

    fn current() -> ScopeTarget {
        ScopeTarget::paths((EntitySearch::Table, EntitySearch::ScopePaths))
    }

    fn history(interval: TimeInterval) -> ScopeTarget {
        ScopeTarget::history((Alias::new("e"), Alias::new("id")), TimeProp::Observed, interval, HistoryBranch::live())
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

    /// A one-term disjunction renders without nesting, so no special case for a
    /// single `OR` term is needed.
    #[test]
    fn a_single_scope_term_renders_without_nesting() {
        let sql = render(condition(&scope("/A/B"), &current()));
        assert!(sql.contains("::lquery"), "{sql}");
        assert!(!sql.contains(" OR "), "{sql}");
        assert_eq!(sql.matches("::lquery").count(), 1, "{sql}");
    }

    #[test]
    fn a_single_scope_history_term_renders_one_probe() {
        let sql = render(condition(&scope("/A/B"), &history(TimeInterval::Unbounded)));
        assert_eq!(sql.matches(r#") AS "sh" WHERE"#).count(), 1, "{sql}");
        assert!(!sql.contains(") OR EXISTS(SELECT 1 FROM (SELECT"), "{sql}");
    }

    #[test]
    fn comma_separated_scopes_render_a_disjunction() {
        let sql = render(condition(&scope("/A,/B"), &current()));
        assert!(sql.contains(" OR "), "{sql}");
    }

    #[test]
    fn semicolon_separated_scopes_render_a_conjunction() {
        let sql = render(condition(&scope("(/A;/B)"), &current()));
        assert!(sql.contains(" AND "), "{sql}");
    }

    #[test]
    fn scope_history_renders_one_probe_per_term() {
        let sql = render(condition(&scope("(/A;/B),/C"), &history(TimeInterval::Unbounded)));
        assert_eq!(sql.matches(r#") AS "sh" WHERE"#).count(), 3, "{sql}");
        assert!(sql.contains("scope_history"), "{sql}");
    }

    /// Clause 4.18: a Scope "is considered valid from the time it has been set
    /// until the time it has been explicitly removed". A `between` window
    /// therefore asks whether the interval overlaps it, never whether the event
    /// instant itself falls inside it, which is what would lose annex C clause
    /// C.5.16's Vehicle B9211.
    #[test]
    fn a_between_window_tests_the_validity_interval_rather_than_the_event_instant() {
        let interval = TimeInterval::Between {
            from: instant("2018-08-01T12:00:00Z"),
            to: instant("2018-08-01T13:00:00Z"),
        };
        let sql = render(condition(&scope("/A"), &history(interval)));
        assert!(sql.contains("\"sh\".\"observed_at\" < "), "{sql}");
        assert!(
            !sql.contains("\"sh\".\"observed_at\" >= "),
            "a scope set before the window is still valid inside it: {sql}"
        );
        assert!(sql.contains("NOT EXISTS"), "{sql}");
    }

    /// Scope history hangs off the temporal catalogue, so the probe reads the
    /// Entity it correlates on inside both branches of the scope source: the
    /// recorded events through the catalogue's identifier, the queued Scope
    /// entries through their own.
    #[test]
    fn scope_history_probes_read_the_entity_inside_each_source() {
        let sql = render(condition(&scope("/A"), &history(TimeInterval::Unbounded)));
        assert!(sql.contains("\"re\".\"id\" = \"e\".\"id\""), "{sql}");
        assert!(sql.contains("\"pe\".\"entity_id\" = \"e\".\"id\""), "{sql}");
        assert!(!sql.contains("\"sh\".\"time\""), "{sql}");
    }

    /// Clause 5.2.21: each `timeproperty` positions a scope event on its own
    /// column, exactly as it positions an attribute instance.
    #[test]
    fn each_time_property_positions_the_scope_probe_on_its_own_column() {
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
                HistoryBranch::live(),
            );
            let sql = render(condition(&scope("/A"), &target));
            assert!(sql.contains(&format!("\"sh\".\"{column}\" IS NOT NULL")), "{sql}");
            assert!(sql.contains(&format!("\"sh_next\".\"{column}\" <= ")), "{sql}");
        }
    }

    /// A removal names no scope path, which is why a query measured on
    /// `deletedAt` selects nothing: every probe still tests a path, and every
    /// row carrying that axis is pathless.
    #[test]
    fn every_scope_probe_tests_a_path_the_row_carries() {
        let sql = render(condition(&scope("/A"), &history(TimeInterval::Unbounded)));
        assert!(sql.contains("\"sh\".\"scope_path\" ~ "), "{sql}");
    }

    /// Both targets walk the same grammar, so a change to the precedence
    /// of `,` and `;` cannot reach one and miss the others.
    #[test]
    fn every_target_renders_the_same_boolean_structure() {
        let scope_q = scope("(/A;/B),/C");
        let current_sql = render(condition(&scope_q, &current()));
        let history_sql = render(condition(&scope_q, &history(TimeInterval::Unbounded)));
        for sql in [&current_sql, &history_sql] {
            assert!(sql.contains(" AND "), "{sql}");
            assert!(sql.contains(" OR "), "{sql}");
            assert_eq!(sql.matches("::lquery").count(), 3, "{sql}");
        }
    }
}
