//! The Temporal Evolutions a temporal read selects, each with the types it is
//! recorded under.
//!
//! Clause 4.5.6 makes the Temporal Evolution the Entity as clause 4.5.1
//! mandates, and the application writes the types of an Entity's latest queued
//! intent stating types over the recorded ones, so on the live branch those
//! types are the Entity's before the drain as after it. An Entity only queued
//! intents stating no type name has no Temporal Evolution yet.
//!
//! The union's recorded branch carries no condition of its own: PostgreSQL
//! flattens a `UNION ALL` into one append relation only when every branch is a
//! bare relation, and only then does an identifier-ordered page read the
//! catalogue's identifier index and stop at the page. The recorded Evolutions
//! a queued intent retypes are therefore left out by a condition over the
//! union, through the flag each branch projects.

use crate::history_source::{HistoryBranch, PendingEntries, pending};
use andromeda_postgresql_schema::temporal::TemporalEntities;
use andromeda_postgresql_seaquery::branch::qualified;
use sea_query::{Condition, Expr, ExprTrait, Iden, IntoIden, Query, SimpleExpr, TableRef, UnionType};

/// Alias `re`: a recorded Temporal Evolution.
#[derive(Iden, Clone, Copy)]
#[iden = "re"]
struct RecordedEvolution;

/// Alias `ru`: one row of the union of recorded and queued Evolutions.
#[derive(Iden, Clone, Copy)]
#[iden = "ru"]
struct EvolutionRow;

/// The flag a union row carries: whether it is a queued intent's.
#[derive(Iden, Clone, Copy)]
#[iden = "queued"]
struct Queued;

/// The Temporal Evolutions of `branch` under `alias`, with the columns `id`
/// and `entity_type_name_keys`.
pub fn entity_rows(branch: &HistoryBranch, alias: impl IntoIden) -> TableRef {
    let schema = branch.schema();
    match branch.pending() {
        PendingEntries::Absent => qualified(schema, TemporalEntities::Table).alias(alias),
        PendingEntries::Unioned => {
            let mut queued = pending::typed_entity_rows();
            queued.expr_as(SimpleExpr::Constant(true.into()), Queued);
            let mut union = Query::select();
            union
                .column((RecordedEvolution, TemporalEntities::Id))
                .column((RecordedEvolution, TemporalEntities::EntityTypeNameKeys))
                .expr_as(SimpleExpr::Constant(false.into()), Queued)
                .from(qualified(schema, TemporalEntities::Table).alias(RecordedEvolution))
                .union(UnionType::All, queued);
            let mut rows = Query::select();
            rows.column((EvolutionRow, TemporalEntities::Id))
                .column((EvolutionRow, TemporalEntities::EntityTypeNameKeys))
                .from_subquery(union.take(), EvolutionRow)
                .cond_where(
                    Condition::any()
                        .add(Expr::col((EvolutionRow, Queued)))
                        .add(pending::types_queued_for(Expr::col((EvolutionRow, TemporalEntities::Id))).not()),
                );
            TableRef::SubQuery(Box::new(rows.take()), alias.into_iden())
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::history_source::{HistoryBranch, entity_rows::entity_rows};
    use sea_query::{Alias, Asterisk, IntoIden, PostgresQueryBuilder, Query};

    fn rendered(source: sea_query::TableRef) -> String {
        Query::select().column(Asterisk).from(source).to_owned().to_string(PostgresQueryBuilder)
    }

    #[test]
    fn pending_entities_take_the_types_of_their_latest_typed_intent() {
        let sql = rendered(entity_rows(&HistoryBranch::live(), Alias::new("e")));
        assert!(
            sql.contains(
                r#"SELECT DISTINCT ON ("pt"."entity_id") "pt"."entity_id" AS "id", "pt"."entity_type_name_keys" FROM "temporal_append_intents" AS "pt" WHERE cardinality("pt"."entity_type_name_keys") > 0 ORDER BY "pt"."entity_id" ASC, "pt"."intent_key" DESC"#
            ),
            "{sql}"
        );
        assert!(
            sql.contains(r#"WHERE "ru"."queued" OR (NOT EXISTS(SELECT 1 FROM "temporal_append_intents" AS "pt" WHERE "pt"."entity_id" = "ru"."id""#),
            "{sql}"
        );
    }

    /// The recorded branch is the bare catalogue, so the planner flattens the
    /// union and an identifier-ordered page reads the identifier index.
    #[test]
    fn the_recorded_branch_carries_no_condition_of_its_own() {
        let sql = rendered(entity_rows(&HistoryBranch::live(), Alias::new("e")));
        assert!(
            sql.contains(r#"(SELECT "re"."id", "re"."entity_type_name_keys", FALSE AS "queued" FROM "temporal_entities" AS "re" UNION ALL"#),
            "{sql}"
        );
    }

    #[test]
    fn a_snapshot_reads_its_catalogue_alone() {
        let sql = rendered(entity_rows(&HistoryBranch::recorded_in(Alias::new("snap_1").into_iden()), Alias::new("e")));
        pretty_assertions::assert_eq!(sql, r#"SELECT * FROM "snap_1"."temporal_entities" AS "e""#);
    }
}
