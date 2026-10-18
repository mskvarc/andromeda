//! The rows of scope history a temporal read takes an Entity's Scope from.
//!
//! A row carries the Entity identifier ([`HistoryGrain::EntityId`]) beside the
//! `scope_history` columns under their own names, so the clause 4.18 validity
//! reading treats a recorded event and a queued one alike. Read for an
//! Evolution by key, it carries the surrogate key
//! ([`HistoryGrain::TemporalEntityKey`]) in place of the identifier.

use crate::history_source::{HistoryBranch, HistoryOf, PendingEntries, entry_values::scope_value_columns, grain::HistoryGrain, pending};
use andromeda_postgresql_schema::temporal::{ScopeHistory, TemporalEntities};
use andromeda_postgresql_seaquery::branch::qualified;
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
        rows.expr_as(Expr::col((RecordedEvent, ScopeHistory::TemporalEntityKey)), HistoryGrain::TemporalEntityKey);
        recorded_event_columns(&mut rows, schema);
        // clone: the key stays with the read, which every source of it shares.
        rows.and_where(Expr::col((RecordedEvent, ScopeHistory::TemporalEntityKey)).eq(key.clone()));
        return TableRef::SubQuery(Box::new(rows.take()), alias.into_iden());
    }
    let pending = branch.pending();
    let mut rows = Query::select();
    rows.expr_as(Expr::col((RecordedEvolution, TemporalEntities::Id)), HistoryGrain::EntityId);
    recorded_event_columns(&mut rows, schema);
    rows.join_as(
        JoinType::Join,
        qualified(schema, TemporalEntities::Table),
        RecordedEvolution,
        Expr::col((RecordedEvolution, TemporalEntities::TemporalEntityKey)).equals((RecordedEvent, ScopeHistory::TemporalEntityKey)),
    );
    of.narrow(&mut rows, Expr::col((RecordedEvolution, TemporalEntities::Id)));
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
    use sea_query::{Alias, Asterisk, Expr, IntoIden, PostgresQueryBuilder, Query};

    fn rendered(source: sea_query::TableRef) -> String {
        Query::select().column(Asterisk).from(source).to_owned().to_string(PostgresQueryBuilder)
    }

    #[test]
    fn the_live_scope_rows_union_the_queued_scope_entries() {
        let sql = rendered(scope_rows(&HistoryBranch::live(), &HistoryOf::EveryEntity, Alias::new("sh")));
        assert!(sql.contains(r#"FROM "scope_history" AS "rsh""#), "{sql}");
        assert!(sql.contains(r#""pe"."scope_path"::ltree AS "scope_path""#), "{sql}");
        assert!(sql.contains(r#""pe"."entry_kind" IN (3, 4)"#), "{sql}");
    }

    /// Clause 4.18: a queued assertion of the Scope already in force sets
    /// nothing, so the read leaves it out exactly as the application will not
    /// record it: judged on both axes against the recorded Scope events of its
    /// Entity and the Scope entries queued before it.
    #[test]
    fn a_queued_assertion_of_the_scope_in_force_is_left_out_of_the_read() {
        let sql = rendered(scope_rows(&HistoryBranch::live(), &HistoryOf::EveryEntity, Alias::new("sh")));
        assert!(
            sql.contains(r#"FROM "scope_history" AS "pj" JOIN "temporal_entities" AS "pjv" ON "pjv"."temporal_entity_key" = "pj"."temporal_entity_key" WHERE "pjv"."id" = "pe"."entity_id""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"FROM "temporal_append_entries" AS "pq" WHERE "pq"."entity_id" = "pe"."entity_id" AND "pq"."intent_key" < "pe"."intent_key" AND "pq"."entry_kind" IN (3, 4)"#),
            "{sql}"
        );
        for axis in ["recorded_at", "observed_at"] {
            assert!(
                sql.contains(&format!(r#"WHERE "sah"."{axis}" <= "pe"."recorded_at" ORDER BY "sah"."{axis}" DESC LIMIT"#)),
                "{axis}: {sql}"
            );
        }
        assert!(
            sql.contains(r#"FROM "temporal_append_entries" AS "pa" WHERE "pa"."intent_key" = "pe"."intent_key" AND "pa"."entry_kind" IN (4)"#),
            "{sql}"
        );
    }

    /// A Change, an Instance and a Removal are events whatever the history
    /// holds, so only the assertion kind is judged.
    #[test]
    fn a_queued_change_an_instance_and_a_removal_are_read_unconditionally() {
        let sql = rendered(scope_rows(&HistoryBranch::live(), &HistoryOf::EveryEntity, Alias::new("sh")));
        assert!(sql.contains(r#"("pe"."entry_kind" <> 4 OR (NOT ("#), "{sql}");
    }

    fn evolution() -> HistoryOf {
        HistoryOf::Evolution(EvolutionColumns {
            id: Expr::col((Alias::new("e"), Alias::new("id"))),
            key: Expr::col((Alias::new("e"), Alias::new("temporal_entity_key"))),
        })
    }

    /// Over recorded rows alone the Scope events of an Evolution are narrowed
    /// by the key each event carries, with no catalogue row read.
    #[test]
    fn the_scope_events_of_an_evolution_over_recorded_rows_are_narrowed_by_key() {
        for (branch, events) in [
            (HistoryBranch::live_recorded(), r#"FROM "scope_history" AS "rsh""#),
            (
                HistoryBranch::recorded_in(Alias::new("snap_1").into_iden()),
                r#"FROM "snap_1"."scope_history" AS "rsh""#,
            ),
        ] {
            let sql = rendered(scope_rows(&branch, &evolution(), Alias::new("sh")));
            assert!(
                sql.starts_with(r#"SELECT * FROM (SELECT "rsh"."temporal_entity_key" AS "temporal_entity_key", "rsh"."recorded_at""#),
                "{sql}"
            );
            assert!(sql.contains(events), "{sql}");
            assert!(
                sql.ends_with(r#"WHERE "rsh"."temporal_entity_key" = "e"."temporal_entity_key") AS "sh""#),
                "{sql}"
            );
            assert!(!sql.contains("temporal_entities") && !sql.contains("entity_id"), "{sql}");
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
        let sql = rendered(scope_rows(
            &HistoryBranch::recorded_in(Alias::new("snap_1").into_iden()),
            &HistoryOf::EveryEntity,
            Alias::new("sh"),
        ));
        assert!(sql.contains(r#"FROM "snap_1"."scope_history" AS "rsh""#), "{sql}");
        assert!(!sql.contains("temporal_append"), "{sql}");
    }
}
