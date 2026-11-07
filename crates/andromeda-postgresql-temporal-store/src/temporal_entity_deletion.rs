//! Temporal Evolution deletion (clause 5.6.16).
//!
//! Deleting the temporal evolution of an entity removes its queued history,
//! its recorded history, the streams that grouped it and the Evolution itself;
//! an Entity with a current state keeps its identity row
//! ([`evolution_vacancy_statement`]).
//!
//! Queued history is removed rather than applied first. Reads count it as
//! recorded, and clause 5.6.16 removes all of it, so removing it leaves what
//! applying it and then deleting it would leave, without the cost of a long
//! queue and without depending on whether every queued write can still apply.
//! Every write that queues history for an Entity holds its identity row's lock
//! until it commits, so once the deletion holds that lock, every such write is
//! either committed, and its history goes with the deletion, or waits, and
//! applies after it. An intent another broker holds under lease goes too: that
//! broker's application then removes fewer intents than it claimed and rolls
//! back.
//!
//! Nothing in the database sequences the removals: the hypertables carry no
//! foreign key back to the catalogue, because a referential action on the
//! broker's hottest write path costs a lookup per inserted row, and no trigger
//! cascades anything. The order is therefore stated here, inside one
//! transaction, and
//! [`audit_orphaned_temporal_rows`](TemporalRepository::audit_orphaned_temporal_rows)
//! is the check that the arrangement held.
//!
//! Core Entity deletion (clause 5.6.6) is a different operation and does not
//! reach this module: it leaves temporal state standing and records the
//! tombstones the specification requires instead.

use crate::{
    error::TemporalStoreError,
    evolution_vacancy::evolution_vacancy_statement,
    history_removal::{remove_scope_history, remove_stream_history},
    repo::TemporalRepository,
    stored_catalog_error::StoredCatalogError,
    stream_census::counted_stream_removal,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_branch::schema_iden;
use andromeda_postgresql_codes::entity_key::EntityKey;
use andromeda_postgresql_query::{
    entity_sides::holds_evolution,
    history_source::{HistoryBranch, PendingEntries, entity_rows::holds_visible_evolution},
};
use andromeda_postgresql_schema::{
    current::Entities,
    queue::TemporalAppendIntents,
    temporal::{AttributeHistory, ScopeHistory, TemporalAttributeStreams},
};
use andromeda_postgresql_seaquery::{aggregate::count_all, branch::qualified, statement::Statement};
use andromeda_postgresql_temporal_query::alias::{Ah, S, Te};
use andromeda_temporal::temporal_not_found_reason::TemporalNotFoundReason;
use sea_query::{DeleteStatement, DynIden, Expr, ExprTrait, Iden, JoinType, LockType, Query, QueryStatementBuilder, SelectStatement, SimpleExpr};
use sqlx::{Postgres, Row, Transaction, postgres::PgRow};

/// The `scope_history` alias in the orphan audit.
#[derive(Iden, Clone, Copy)]
#[iden = "sh"]
struct Sh;

/// The count of attribute-history rows whose stream is gone.
#[derive(Iden, Clone, Copy)]
#[iden = "orphaned_attribute_history_rows"]
struct OrphanedAttributeHistoryRows;

/// The count of scope-history rows whose temporal entity is gone.
#[derive(Iden, Clone, Copy)]
#[iden = "orphaned_scope_history_rows"]
struct OrphanedScopeHistoryRows;

/// The count of streams whose temporal entity is gone.
#[derive(Iden, Clone, Copy)]
#[iden = "orphaned_streams"]
struct OrphanedStreams;

/// What one Temporal Evolution deletion removed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TemporalEvolutionDeletion {
    /// The temporal entity the deletion resolved and removed.
    pub entity_key: EntityKey,
    /// Queued intents of the entity removed, zero on a Snapshot branch, which
    /// queues nothing.
    pub queued_intents: u64,
    /// Attribute history rows removed.
    pub attribute_history_rows: u64,
    /// Scope history rows removed.
    pub scope_history_rows: u64,
    /// Attribute streams removed.
    pub streams: u64,
}

/// Temporal rows whose owning row no longer exists.
///
/// Every count is expected to be zero. A non-zero count means a deletion did
/// not run its full sequence, which leaves history no query can reach and no
/// deletion can address.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OrphanedTemporalRows {
    /// Attribute history rows whose `stream_key` names no stream.
    pub attribute_history_rows: i64,
    /// Scope history rows whose Entity holds no Temporal Evolution.
    pub scope_history_rows: i64,
    /// Streams whose Entity holds no Temporal Evolution.
    pub streams: i64,
}

impl TemporalRepository {
    /// Delete the whole temporal evolution of one entity (clause 5.6.16), its
    /// queued history included.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::NotFound`] when no Entity carrying the
    /// identifier holds a Temporal Evolution, recorded or brought into being
    /// by its queued history, [`TemporalStoreError::Database`] when a
    /// statement fails, and [`TemporalStoreError::StoredCatalog`] when the
    /// stored temporal-entity key is outside the range the database assigns.
    pub async fn delete_temporal_evolution(&self, target: &ContextTarget, entity_id: &Identifier) -> Result<TemporalEvolutionDeletion, TemporalStoreError> {
        let mut tx = self.pool.begin().await?;
        let branch = deleted_branch(target);
        let schema = branch.schema();
        let entity_key = locate_temporal_entity(&mut tx, &branch, entity_id).await?;
        let queued_intents = remove_queued_history(&mut tx, &branch, entity_id).await?;
        let stream_keys: Vec<i64> = owned_streams_statement(schema, entity_key).to_query_scalar().fetch_all(&mut *tx).await?;
        let attribute_history_rows = remove_stream_history(&mut tx, schema, &stream_keys).await?;
        let scope_history_rows = remove_scope_history(&mut tx, schema, entity_key).await?;
        let streams: i64 = counted_stream_removal(schema, delete_attribute_streams_statement(schema, entity_key))
            .map_err(TemporalStoreError::StatementBuild)?
            .to_query_scalar()
            .fetch_one(&mut *tx)
            .await?;
        let streams = streams.unsigned_abs();
        let addressed = Query::select().expr(Expr::val(entity_key.get())).take();
        evolution_vacancy_statement(schema, addressed).to_query().execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(TemporalEvolutionDeletion {
            entity_key,
            queued_intents,
            attribute_history_rows,
            scope_history_rows,
            streams,
        })
    }

    /// Report temporal rows left behind by an incomplete deletion sequence.
    ///
    /// The audit stands in for what no constraint enforces: the hypertables
    /// carry no foreign key, and an identity row keeps its streams' foreign
    /// keys satisfied after its Temporal Evolution is given up. An empty
    /// report, every count zero, is the expected outcome.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when the audit statement fails
    /// and when a returned count does not carry the expected type.
    pub async fn audit_orphaned_temporal_rows(&self) -> Result<OrphanedTemporalRows, TemporalStoreError> {
        let row = audit_orphaned_statement().to_query().fetch_one(&self.pool).await?;
        Ok(OrphanedTemporalRows {
            attribute_history_rows: count(&row, "orphaned_attribute_history_rows")?,
            scope_history_rows: count(&row, "orphaned_scope_history_rows")?,
            streams: count(&row, "orphaned_streams")?,
        })
    }
}

/// Read one audit count; a column that does not decode is a `sqlx` failure
/// naming it.
fn count(row: &PgRow, column: &str) -> Result<i64, TemporalStoreError> {
    Ok(row.try_get(column)?)
}

/// The branch a deletion of `target` removes history from: the live branch
/// with its queue, or a Snapshot branch, which has none.
fn deleted_branch(target: &ContextTarget) -> HistoryBranch {
    match schema_iden(target) {
        Some(schema) => HistoryBranch::recorded_in(schema),
        None => HistoryBranch::live(),
    }
}

/// Lock the identity row one deletion addresses and resolve its key, when it
/// holds a Temporal Evolution a read of `branch` sees.
///
/// The row is locked before the Evolution is judged, in a statement of its
/// own: the judgement's snapshot then starts after every write that queued
/// history for the Entity before the lock was granted has committed.
async fn locate_temporal_entity(tx: &mut Transaction<'_, Postgres>, branch: &HistoryBranch, entity_id: &Identifier) -> Result<EntityKey, TemporalStoreError> {
    let not_found = || TemporalStoreError::NotFound(TemporalNotFoundReason::Entity { id: entity_id.clone() }); // clone: the reason owns the entity it names
    let located = lock_identity_row_statement(branch.schema(), entity_id.as_str())
        .to_query()
        .fetch_optional(&mut **tx)
        .await?;
    let Some(row) = located else {
        return Err(not_found());
    };
    let stored: i64 = row.try_get(Iden::to_string(&Entities::EntityKey).as_str())?;
    let holds: bool = visible_evolution_statement(branch, stored).to_query_scalar().fetch_one(&mut **tx).await?;
    if !holds {
        return Err(not_found());
    }
    Ok(EntityKey::try_from(stored).map_err(StoredCatalogError::EntityKey)?)
}

/// Lock the identity row of the addressed Entity before anything is judged or
/// removed.
fn lock_identity_row_statement(schema: Option<&DynIden>, id: &str) -> SelectStatement {
    Query::select()
        .column(Entities::EntityKey)
        .from(qualified(schema, Entities::Table))
        .and_where(Expr::col(Entities::Id).eq(id))
        .lock(LockType::Update)
        .to_owned()
}

/// Whether the locked identity row holds a Temporal Evolution a read of
/// `branch` sees. Clause 5.6.16 addresses an Entity's Temporal Evolution: a
/// row holding a current state alone, or queued history stating no type, is
/// not one.
fn visible_evolution_statement(branch: &HistoryBranch, entity_key: i64) -> SelectStatement {
    Query::select()
        .expr(holds_visible_evolution(branch, Entities::Table))
        .from(qualified(branch.schema(), Entities::Table))
        .and_where(Expr::col(Entities::EntityKey).eq(entity_key))
        .to_owned()
}

/// Remove every queued intent of the Entity on a branch with a queue,
/// reporting how many.
async fn remove_queued_history(tx: &mut Transaction<'_, Postgres>, branch: &HistoryBranch, entity_id: &Identifier) -> Result<u64, TemporalStoreError> {
    match branch.pending() {
        PendingEntries::Absent => Ok(0),
        PendingEntries::Unioned => Ok(queued_history_removal_statement(entity_id.as_str())
            .to_query()
            .execute(&mut **tx)
            .await?
            .rows_affected()),
    }
}

/// Remove the queued intents of one Entity, every entry of each going with
/// it.
fn queued_history_removal_statement(id: &str) -> DeleteStatement {
    Query::delete()
        .from_table(TemporalAppendIntents::Table)
        .and_where(Expr::col(TemporalAppendIntents::EntityId).eq(id))
        .to_owned()
}

/// The streams of one temporal entity, whose history the deletion removes by
/// their keys.
fn owned_streams_statement(schema: Option<&DynIden>, entity_key: EntityKey) -> SelectStatement {
    Query::select()
        .column(TemporalAttributeStreams::StreamKey)
        .from(qualified(schema, TemporalAttributeStreams::Table))
        .and_where(Expr::col(TemporalAttributeStreams::EntityKey).eq(entity_key.get()))
        .to_owned()
}

/// Remove one entity's attribute streams, reporting how many the deletion
/// removed. An Entity whose current state keeps its identity row keeps no
/// stream: the Evolution goes whole.
fn delete_attribute_streams_statement(schema: Option<&DynIden>, entity_key: EntityKey) -> DeleteStatement {
    Query::delete()
        .from_table(qualified(schema, TemporalAttributeStreams::Table))
        .and_where(Expr::col(TemporalAttributeStreams::EntityKey).eq(entity_key.get()))
        .to_owned()
}

/// The orphan-count subquery of one dangling reference: rows of `child` whose
/// `parent` no longer exists, via a left join that keeps the unmatched rows.
fn orphan_count(child: SelectStatement) -> SimpleExpr {
    SimpleExpr::SubQuery(None, Box::new(child.into_sub_query_statement()))
}

/// The audit of what no constraint enforces: one count of history rows whose
/// stream is gone, and one each of Scope rows and streams whose Entity holds
/// no Temporal Evolution, its identity row gone or holding a current state
/// alone.
fn audit_orphaned_statement() -> SelectStatement {
    let orphaned_history = Query::select()
        .expr(count_all())
        .from_as(AttributeHistory::Table, Ah)
        .join_as(
            JoinType::LeftJoin,
            TemporalAttributeStreams::Table,
            S,
            Expr::col((S, TemporalAttributeStreams::StreamKey)).equals((Ah, AttributeHistory::StreamKey)),
        )
        .and_where(Expr::col((S, TemporalAttributeStreams::StreamKey)).is_null())
        .to_owned();
    let orphaned_scope = Query::select()
        .expr(count_all())
        .from_as(ScopeHistory::Table, Sh)
        .join_as(
            JoinType::LeftJoin,
            Entities::Table,
            Te,
            Expr::col((Te, Entities::EntityKey)).equals((Sh, ScopeHistory::EntityKey)),
        )
        .and_where(holds_evolution(Te).not())
        .to_owned();
    let orphaned_streams = Query::select()
        .expr(count_all())
        .from_as(TemporalAttributeStreams::Table, S)
        .join_as(
            JoinType::LeftJoin,
            Entities::Table,
            Te,
            Expr::col((Te, Entities::EntityKey)).equals((S, TemporalAttributeStreams::EntityKey)),
        )
        .and_where(holds_evolution(Te).not())
        .to_owned();
    Query::select()
        .expr_as(orphan_count(orphaned_history), OrphanedAttributeHistoryRows)
        .expr_as(orphan_count(orphaned_scope), OrphanedScopeHistoryRows)
        .expr_as(orphan_count(orphaned_streams), OrphanedStreams)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::temporal_entity_deletion::{
        audit_orphaned_statement,
        delete_attribute_streams_statement,
        deleted_branch,
        lock_identity_row_statement,
        owned_streams_statement,
        queued_history_removal_statement,
        visible_evolution_statement,
    };
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_postgresql_codes::entity_key::EntityKey;
    use andromeda_postgresql_query::history_source::{HistoryBranch, PendingEntries};
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, IntoIden, PostgresQueryBuilder};

    fn key() -> EntityKey {
        EntityKey::try_from(1).expect("a positive key")
    }

    /// The row is locked whatever it holds, so the judgement of its Evolution
    /// reads every intent a writer committed before the lock was granted.
    #[test]
    fn the_addressed_identity_row_is_locked_before_its_evolution_is_judged() {
        assert_eq!(
            lock_identity_row_statement(None, "urn:ngsi-ld:Vehicle:1").to_string(PostgresQueryBuilder),
            r#"SELECT "entity_key" FROM "entities" WHERE "id" = 'urn:ngsi-ld:Vehicle:1' FOR UPDATE"#
        );
    }

    /// Reads count a Temporal Evolution queued history brings into being as
    /// recorded, so clause 5.6.16 deletes it as one.
    #[test]
    fn a_live_deletion_addresses_an_evolution_recorded_or_brought_into_being_by_queued_history() {
        assert_eq!(
            visible_evolution_statement(&HistoryBranch::live(), 7).to_string(PostgresQueryBuilder),
            concat!(
                r#"SELECT "entities"."evolution_created_at" IS NOT NULL OR EXISTS(SELECT 1 FROM "temporal_append_intents" AS "pt" "#,
                r#"WHERE "pt"."entity_id" = "entities"."id" AND cardinality("pt"."entity_type_name_keys") > 0) "#,
                r#"FROM "entities" WHERE "entity_key" = 7"#
            )
        );
    }

    #[test]
    fn a_snapshot_deletion_addresses_a_recorded_evolution_alone() {
        let branch = HistoryBranch::recorded_in(Alias::new("snap_1").into_iden());
        assert_eq!(
            visible_evolution_statement(&branch, 7).to_string(PostgresQueryBuilder),
            r#"SELECT "entities"."evolution_created_at" IS NOT NULL FROM "snap_1"."entities" WHERE "entity_key" = 7"#
        );
    }

    #[test]
    fn a_live_deletion_removes_from_the_branch_with_the_queue() {
        assert_eq!(deleted_branch(&ContextTarget::Live).pending(), PendingEntries::Unioned);
    }

    /// Clause 5.6.16 removes the whole Temporal Evolution, so every queued
    /// intent of the Entity goes, whether or not it states types.
    #[test]
    fn every_queued_intent_of_the_entity_is_removed() {
        assert_eq!(
            queued_history_removal_statement("urn:ngsi-ld:Vehicle:1").to_string(PostgresQueryBuilder),
            r#"DELETE FROM "temporal_append_intents" WHERE "entity_id" = 'urn:ngsi-ld:Vehicle:1'"#
        );
    }

    #[test]
    fn the_history_is_addressed_through_the_keys_of_the_streams_the_entity_owns() {
        assert_eq!(
            owned_streams_statement(None, key()).to_string(PostgresQueryBuilder),
            r#"SELECT "stream_key" FROM "temporal_attribute_streams" WHERE "entity_key" = 1"#
        );
    }

    #[test]
    fn the_sequence_removes_the_streams_the_evolution_occupies() {
        assert!(
            delete_attribute_streams_statement(None, key())
                .to_string(PostgresQueryBuilder)
                .starts_with(r#"DELETE FROM "temporal_attribute_streams""#)
        );
    }

    #[test]
    fn the_audit_counts_the_temporal_rows_no_evolution_holds() {
        let sql = audit_orphaned_statement().to_string(PostgresQueryBuilder);
        for column in ["orphaned_attribute_history_rows", "orphaned_scope_history_rows", "orphaned_streams"] {
            assert!(sql.contains(column), "missing {column}: {sql}");
        }
        assert_eq!(sql.matches("COUNT(*)").count(), 3, "{sql}");
        assert_eq!(sql.matches(r#"WHERE NOT "te"."evolution_created_at" IS NOT NULL"#).count(), 2, "{sql}");
    }

    #[test]
    fn no_statement_records_a_tombstone_for_a_temporal_evolution_deletion() {
        let sql = delete_attribute_streams_statement(None, key()).to_string(PostgresQueryBuilder);
        assert!(!sql.contains("INSERT"), "{sql}");
        assert!(!sql.contains("deleted_at"), "{sql}");
    }
}
