//! The rows of scope history a temporal read takes an Entity's Scope from.
//!
//! A row carries the Entity identifier ([`HistoryGrain::EntityId`]) beside the
//! `scope_history` columns under their own names, so the clause 4.18 validity
//! reading treats a recorded event and a queued one alike. Read for an
//! Evolution by key, it carries the surrogate key
//! ([`HistoryGrain::EntityKey`]) in place of the identifier.

use crate::history_source::{HistoryBranch, HistoryOf, PendingEntries, pending};
use andromeda_postgresql_schema::{current::Entities, temporal::ScopeHistory};
use andromeda_postgresql_seaquery::branch::qualified;
use andromeda_postgresql_stored_form::{history_entry_values::scope_value_columns, history_grain::HistoryGrain};
use sea_query::{DynIden, Expr, ExprTrait, Iden, IntoIden, JoinType, Query, SelectStatement, TableRef, UnionType};

/// Alias `rsh`: a recorded scope event.
#[derive(Iden, Clone, Copy)]
#[iden = "rsh"]
struct RecordedEvent;

/// Alias `re`: a recorded Temporal Evolution.
#[derive(Iden, Clone, Copy)]
#[iden = "re"]
struct RecordedEvolution;

/// The identity columns of a scope event, ahead of its value columns.
const IDENTITY_COLUMNS: [ScopeHistory; 3] = [ScopeHistory::RecordedAt, ScopeHistory::WriteId, ScopeHistory::WriteOrdinal];

/// The scope history `of` reads in `branch`, under `alias`: its recorded
/// events and, where the branch reads its queue, the queued Scope entries the
/// application would write.
pub fn scope_rows(branch: &HistoryBranch, of: &HistoryOf, alias: impl IntoIden) -> TableRef {
    let schema = branch.schema();
    if let Some(key) = branch.evolution_key(of) {
        let mut rows = Query::select();
        rows.expr_as(Expr::col((RecordedEvent, ScopeHistory::EntityKey)), HistoryGrain::EntityKey);
        recorded_event_columns(&mut rows, schema);
        // clone: the key stays with the read, which every source of it shares.
        rows.and_where(Expr::col((RecordedEvent, ScopeHistory::EntityKey)).eq(key.clone()));
        return TableRef::SubQuery(Box::new(rows.take()), alias.into_iden());
    }
    let pending = branch.pending();
    let mut rows = Query::select();
    rows.expr_as(Expr::col((RecordedEvolution, Entities::Id)), HistoryGrain::EntityId);
    recorded_event_columns(&mut rows, schema);
    rows.join_as(
        JoinType::Join,
        qualified(schema, Entities::Table),
        RecordedEvolution,
        Expr::col((RecordedEvolution, Entities::EntityKey)).equals((RecordedEvent, ScopeHistory::EntityKey)),
    );
    of.narrow(&mut rows, Expr::col((RecordedEvolution, Entities::Id)));
    match pending {
        PendingEntries::Unioned => {
            rows.union(UnionType::All, pending::scope_rows(schema, of));
        }
        PendingEntries::Absent => {}
    }
    TableRef::SubQuery(Box::new(rows.take()), alias.into_iden())
}

/// The identity and value columns of the recorded events of the branch
/// `schema` names.
fn recorded_event_columns(select: &mut SelectStatement, schema: Option<&DynIden>) {
    select
        .exprs(IDENTITY_COLUMNS.map(|column| Expr::col((RecordedEvent, column))))
        .exprs(scope_value_columns().map(|column| Expr::col((RecordedEvent, column))))
        .from(qualified(schema, ScopeHistory::Table).alias(RecordedEvent));
}

#[cfg(test)]
mod tests {
    use crate::history_source::{EvolutionColumns, HistoryBranch, HistoryOf, scope_rows::scope_rows};
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use sea_query::{Alias, Asterisk, Expr, IntoIden, Query, SelectStatement};

    fn rendered(source: sea_query::TableRef) -> SelectStatement {
        Query::select().column(Asterisk).from(source).to_owned()
    }

    #[test]
    fn the_live_scope_rows_union_the_queued_scope_entries() {
        let statement = rendered(scope_rows(&HistoryBranch::live(), &HistoryOf::EveryEntity, Alias::new("sh")));
        assert_statement_snapshot!("scope-rows-the-live-scope-rows-union-the-queued-scope-entries", statement);
    }

    /// Clause 4.18: a queued assertion of the Scope already in force sets
    /// nothing, so the read leaves it out exactly as the application will not
    /// record it: judged on both axes against the recorded Scope events of its
    /// Entity and the Scope entries queued before it.
    #[test]
    fn a_queued_assertion_of_the_scope_in_force_is_left_out_of_the_read() {
        let statement = rendered(scope_rows(&HistoryBranch::live(), &HistoryOf::EveryEntity, Alias::new("sh")));
        assert_statement_snapshot!("scope-rows-a-queued-assertion-of-the-scope-in-force-is-left-out-of-the-read", statement);
    }

    /// A Change, an Instance and a Removal are events whatever the history
    /// holds, so only the assertion kind is judged.
    #[test]
    fn a_queued_change_an_instance_and_a_removal_are_read_unconditionally() {
        let statement = rendered(scope_rows(&HistoryBranch::live(), &HistoryOf::EveryEntity, Alias::new("sh")));
        assert_statement_snapshot!("scope-rows-a-queued-change-an-instance-and-a-removal-are-read-unconditionally", statement);
    }

    fn evolution() -> HistoryOf {
        HistoryOf::Evolution(EvolutionColumns {
            id: Expr::col((Alias::new("e"), Alias::new("id"))),
            key: Expr::col((Alias::new("e"), Alias::new("entity_key"))),
        })
    }

    /// Over recorded rows alone the Scope events of an Evolution are narrowed
    /// by the key each event carries, with no catalogue row read.
    #[test]
    fn the_scope_events_of_an_evolution_over_recorded_rows_are_narrowed_by_key() {
        for (index, branch) in [HistoryBranch::live_recorded(), HistoryBranch::recorded_in(Alias::new("snap_1").into_iden())]
            .into_iter()
            .enumerate()
        {
            let statement = rendered(scope_rows(&branch, &evolution(), Alias::new("sh")));
            assert_statement_snapshot!(
                format!(
                    "scope-rows-the-scope-events-of-an-evolution-over-recorded-rows-are-narrowed-by-key-{}",
                    index + 1
                ),
                statement
            );
        }
    }

    /// Over a queue the Scope events of an Evolution are narrowed by its
    /// identifier in both parts.
    #[test]
    fn the_scope_events_of_an_evolution_over_a_queue_are_narrowed_by_identifier() {
        let of = HistoryOf::Entity(Expr::col((Alias::new("e"), Alias::new("id"))));
        pretty_assertions::assert_eq!(
            rendered(scope_rows(&HistoryBranch::live(), &evolution(), Alias::new("sh"))),
            rendered(scope_rows(&HistoryBranch::live(), &of, Alias::new("sh")))
        );
    }

    #[test]
    fn a_snapshot_reads_its_recorded_scope_events_alone() {
        let statement = rendered(scope_rows(
            &HistoryBranch::recorded_in(Alias::new("snap_1").into_iden()),
            &HistoryOf::EveryEntity,
            Alias::new("sh"),
        ));
        assert_statement_snapshot!("scope-rows-a-snapshot-reads-its-recorded-scope-events-alone", statement);
    }
}
