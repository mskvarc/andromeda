//! The queued entries of the live branch that replace a recorded instance.
//!
//! An attribute entry carrying a client `instanceId` replaces the recorded row
//! of that instance on any dataset of the attribute when its intent is applied
//! (clause 4.5.7), and a later queued entry naming the same instance replaces
//! it in turn. A read of the live branch while its queue holds entries leaves
//! the replaced rows out, so it answers as it will once the drain applied them.

use crate::history_source::{
    HistoryBranch,
    PendingEntries,
    grain::HistoryGrain,
    pending::{ATTRIBUTE_KINDS, entry_kind_in},
};
use andromeda_postgresql_schema::{queue::TemporalAppendEntries, temporal_intent::TemporalEntry};
use andromeda_postgresql_seaquery::literal::int_literal;
use sea_query::{Condition, Expr, ExprTrait, Iden, Query, SelectStatement, SimpleExpr};

/// Alias `pr`: a queued entry that replaces another row.
#[derive(Iden, Clone, Copy)]
#[iden = "pr"]
pub(crate) struct ReplacingEntry;

/// The queued entries naming the client `instanceId` `verbatim` of the
/// attribute `attribute_key` of the Entity `entity_id`.
pub(crate) fn naming_instance(entity_id: SimpleExpr, attribute_key: SimpleExpr, verbatim: SimpleExpr) -> SelectStatement {
    let mut naming = Query::select();
    naming
        .expr(int_literal(1))
        .from_as(TemporalAppendEntries::Table, ReplacingEntry)
        .and_where(Expr::col((ReplacingEntry, TemporalAppendEntries::EntityId)).eq(entity_id))
        .and_where(Expr::col((ReplacingEntry, TemporalEntry::AttributeNameKey)).eq(attribute_key))
        .and_where(Expr::col((ReplacingEntry, TemporalEntry::VerbatimInstanceId)).eq(verbatim))
        .and_where(entry_kind_in(ReplacingEntry, &ATTRIBUTE_KINDS));
    naming
}

/// Whether a recorded row of the attribute `attribute_key` of the Entity
/// `entity_id` carrying the client `instanceId` `verbatim` stays beside the
/// queued entries: it carries no client `instanceId`, or no queued entry names
/// its instance. `None` when `pending` reads none.
#[must_use]
pub fn kept_beside_pending(pending: PendingEntries, entity_id: SimpleExpr, attribute_key: SimpleExpr, verbatim: SimpleExpr) -> Option<Condition> {
    match pending {
        PendingEntries::Unioned => {
            // clone: the identifier is tested for absence and then compared.
            let naming = naming_instance(entity_id, attribute_key, verbatim.clone());
            Some(Condition::any().add(verbatim.is_null()).add(Expr::exists(naming).not()))
        }
        PendingEntries::Absent => None,
    }
}

/// Every attribute of an Entity whose recorded rows a queued entry replaces,
/// each once, under the grain names [`HistoryGrain::EntityId`] and
/// [`HistoryGrain::AttributeNameKey`]. `None` where `branch` reads no queue.
#[must_use]
pub fn queued_instance_replacements(branch: &HistoryBranch) -> Option<SelectStatement> {
    match branch.pending() {
        PendingEntries::Unioned => {
            let mut grains = Query::select();
            grains
                .distinct()
                .expr_as(Expr::col((ReplacingEntry, TemporalAppendEntries::EntityId)), HistoryGrain::EntityId)
                .expr_as(Expr::col((ReplacingEntry, TemporalEntry::AttributeNameKey)), HistoryGrain::AttributeNameKey)
                .from_as(TemporalAppendEntries::Table, ReplacingEntry)
                .and_where(Expr::col((ReplacingEntry, TemporalEntry::VerbatimInstanceId)).is_not_null())
                .and_where(entry_kind_in(ReplacingEntry, &ATTRIBUTE_KINDS));
            Some(grains)
        }
        PendingEntries::Absent => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::history_source::{
        HistoryBranch,
        PendingEntries,
        instance_replacement::{kept_beside_pending, queued_instance_replacements},
    };
    use sea_query::{Alias, Expr, IntoIden, PostgresQueryBuilder, Query};

    #[test]
    fn a_recorded_row_stays_unless_a_queued_entry_names_its_client_instance() {
        let kept = kept_beside_pending(
            PendingEntries::Unioned,
            Expr::col(Alias::new("id")),
            Expr::col(Alias::new("key")),
            Expr::col(Alias::new("verbatim")),
        )
        .map(|condition| Query::select().cond_where(condition).to_owned().to_string(PostgresQueryBuilder));
        let sql = kept.expect("a branch reading its queue keeps rows by condition");
        assert!(
            sql.contains(r#""verbatim" IS NULL OR (NOT EXISTS(SELECT 1 FROM "temporal_append_entries" AS "pr""#),
            "{sql}"
        );
        assert!(sql.contains(r#""pr"."verbatim_instance_id" = "verbatim""#), "{sql}");
        assert!(
            kept_beside_pending(
                PendingEntries::Absent,
                Expr::col(Alias::new("id")),
                Expr::col(Alias::new("key")),
                Expr::col(Alias::new("verbatim"))
            )
            .is_none()
        );
    }

    /// Clause 4.5.7: only an entry carrying a client `instanceId` replaces a
    /// recorded row, and only an attribute entry carries one.
    #[test]
    fn the_replaced_attributes_are_those_a_queued_client_instance_names() {
        let sql = queued_instance_replacements(&HistoryBranch::live()).map(|grains| grains.to_string(PostgresQueryBuilder));
        pretty_assertions::assert_eq!(
            sql.as_deref(),
            Some(concat!(
                r#"SELECT DISTINCT "pr"."entity_id" AS "entity_id", "pr"."attribute_name_key" AS "attribute_name_key" FROM "temporal_append_entries" AS "pr""#,
                r#" WHERE "pr"."verbatim_instance_id" IS NOT NULL AND "pr"."entry_kind" IN (1, 2)"#
            ))
        );
        for branch in [HistoryBranch::live_recorded(), HistoryBranch::recorded_in(Alias::new("snap_1").into_iden())] {
            assert!(queued_instance_replacements(&branch).is_none());
        }
    }
}
