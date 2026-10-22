//! Authoritative global reset and complete prefix-owned cleanup.
//!
//! Every mutable tenant table is truncated between test cases. Four tables are
//! deliberately absent. `attribute_names` and `entity_type_names` are
//! dictionaries whose keys are cached in-process, so truncating either would
//! strand every cached key. `schema_metadata` carries the schema contract the
//! broker stamps once after migrating and verifies on every later connection to
//! the tenant: removing the singleton makes the tenant read as never-stamped,
//! and every connection after the reset is refused as drift.
//! `context_source_change_clock` is a singleton a migration seeds and every
//! registration write advances: emptying it would leave those writes nothing to
//! advance, and a revision only has to grow, never to restart.
//!
//! `attribute_history_instance_claims` is named explicitly rather than left to
//! the cascade from `temporal_entities`. It carries the clause 4.5.7 uniqueness
//! of a client-supplied `instanceId`, which `attribute_history` cannot carry
//! itself, so a claim surviving the reset would refuse an identifier the next
//! test case is entitled to use, and relying on a cascade to reclaim it makes
//! that depend on a foreign key rather than on the list. The same holds for
//! `registration_delivery_state`, `subscription_companions`,
//! `forwarded_subscriptions`, `subscription_notified_registrations`,
//! `snapshot_notification_outbox`, `entity_map_entries` and `entity_map_links`:
//! each is also reclaimed by a cascade from the row it hangs off, and each is
//! named so the reset does not depend on that edge.
//!
//! The reset is one transaction that takes every lock it needs before it
//! changes anything: the reset relations, every relation `TRUNCATE ... CASCADE`
//! reaches from them through a foreign key, and every table of every Snapshot
//! branch schema, in one `LOCK TABLE ... NOWAIT`. A broker transaction still
//! open on any of them makes the lock fail at once with `lock_not_available`
//! (55P03) while holding nothing, and the harness runs the reset again once that
//! transaction has finished. The reset therefore never waits for a lock while
//! holding another, so it can never form a deadlock with a broker's expiry sweep
//! or notification drain, and never makes PostgreSQL abort a broker transaction
//! as the victim of one.
//!
//! The live branch is the default schema, reset by the truncation. Each Snapshot
//! branch is its own schema `snap_<key>` (and `snap_<key>_stg` while
//! preparing), test-owned and unreachable from the control tables, so every such
//! schema is dropped after the control rows that named them are gone.

use crate::{
    database_isolation::{DatabaseIsolationError, SerializedLease},
    discovery_census::{CensusError, rebuild_live_census},
    identifiers::TestIdentifiers,
};
use andromeda_postgresql_pool::schema_change_lock::is_lock_not_available;
use andromeda_postgresql_schema::{
    census::{AttributeCensus, EntityTypeAttributeCensus, EntityTypeCensus},
    csr::{Csr, RegistrationDeliveryState, RegistrationInformation},
    current::{CurrentAttributes, Entities, EntitySearch, EntityTypeMembers},
    entity_map::{EntityMapEntries, EntityMapLinks, EntityMaps},
    jsonld::JsonldContexts,
    notification::{NotificationDeliveries, NotificationEvents},
    queue::{PendingSideEffects, TemporalAppendIntents, WriteRecords},
    snapshot::{SnapshotNotificationOutbox, SnapshotPreparation, Snapshots},
    subscription_forwarding::{ForwardedSubscriptions, SubscriptionCompanions},
    subscriptions::{SubscriptionDeliveryState, SubscriptionNotifiedRegistrations, Subscriptions},
    temporal::{
        AttributeHistory,
        AttributeHistoryInstanceClaims,
        ScopeHistory,
        TemporalAttributeStreams,
        TemporalEntities,
        TemporalEntityTypeMembers,
        TemporalStreamShapes,
    },
};
use andromeda_postgresql_seaquery::{
    command::Command,
    introspection::table_is_visible,
    literal::text_literal,
    relation_lock::RelationLock,
    relation_truncation::RelationTruncation,
    schema_definition::SchemaDefinition,
    statement::Statement,
    system_catalog::{PgClass, PgConstraint, PgNamespace},
};
use sea_query::{
    Alias,
    CommonTableExpression,
    DeleteStatement,
    DynIden,
    Expr,
    ExprTrait,
    Iden,
    IntoIden,
    JoinType,
    Query,
    SelectStatement,
    TableName,
    UnionType,
    WithClause,
    WithQuery,
};
use sqlx::{Connection, PgConnection, PgPool, Row};
use std::time::Duration;
use thiserror::Error;
use tokio::time::{Instant, sleep};

/// The relations the reset empties, in the order they are truncated.
fn reset_relations() -> Vec<DynIden> {
    vec![
        WriteRecords::Table.into_iden(),
        TemporalAppendIntents::Table.into_iden(),
        PendingSideEffects::Table.into_iden(),
        CurrentAttributes::Table.into_iden(),
        NotificationDeliveries::Table.into_iden(),
        NotificationEvents::Table.into_iden(),
        ScopeHistory::Table.into_iden(),
        SnapshotNotificationOutbox::Table.into_iden(),
        SnapshotPreparation::Table.into_iden(),
        Snapshots::Table.into_iden(),
        ForwardedSubscriptions::Table.into_iden(),
        SubscriptionCompanions::Table.into_iden(),
        RegistrationInformation::Table.into_iden(),
        RegistrationDeliveryState::Table.into_iden(),
        Csr::Table.into_iden(),
        SubscriptionNotifiedRegistrations::Table.into_iden(),
        SubscriptionDeliveryState::Table.into_iden(),
        Subscriptions::Table.into_iden(),
        AttributeHistoryInstanceClaims::Table.into_iden(),
        AttributeHistory::Table.into_iden(),
        TemporalStreamShapes::Table.into_iden(),
        TemporalAttributeStreams::Table.into_iden(),
        TemporalEntityTypeMembers::Table.into_iden(),
        TemporalEntities::Table.into_iden(),
        EntityTypeMembers::Table.into_iden(),
        EntityTypeCensus::Table.into_iden(),
        EntityTypeAttributeCensus::Table.into_iden(),
        AttributeCensus::Table.into_iden(),
        EntitySearch::Table.into_iden(),
        Entities::Table.into_iden(),
        EntityMapLinks::Table.into_iden(),
        EntityMapEntries::Table.into_iden(),
        EntityMaps::Table.into_iden(),
        JsonldContexts::Table.into_iden(),
    ]
}

/// The pattern every Snapshot branch schema name matches, `snap_<key>` and
/// `snap_<key>_stg` alike.
const BRANCH_SCHEMA_PATTERN: &str = r"snap\_%";

/// The recursive set of relation oids the truncation reaches.
#[derive(Iden, Clone, Copy)]
#[iden = "reached"]
struct Reached;

/// The oids of the tables of every Snapshot branch schema.
#[derive(Iden, Clone, Copy)]
#[iden = "branch_tables"]
struct BranchTables;

/// The column of a set of relation oids.
#[derive(Iden, Clone, Copy)]
#[iden = "relation"]
struct Relation;

/// The `pg_class` row of a relation, `c`.
#[derive(Iden, Clone, Copy)]
#[iden = "c"]
struct RelationRow;

/// The `pg_namespace` row of a schema, `n`.
#[derive(Iden, Clone, Copy)]
#[iden = "n"]
struct SchemaRow;

/// The `pg_constraint` row of a constraint, `k`.
#[derive(Iden, Clone, Copy)]
#[iden = "k"]
struct ConstraintRow;

/// Every relation the reset locks: the reset relations, every relation a
/// foreign key reaches from them, and every table of every Snapshot branch,
/// each as its schema and name.
fn lock_set_statement() -> WithQuery {
    let names: Vec<String> = reset_relations().iter().map(ToString::to_string).collect();
    let mut roots = Query::select();
    roots
        .expr_as(Expr::col((RelationRow, PgClass::Oid)), Relation)
        .from_as(PgClass::Table, RelationRow)
        .and_where(Expr::col((RelationRow, PgClass::Relname)).is_in(names))
        .and_where(table_is_visible(Expr::col((RelationRow, PgClass::Oid))));
    let mut referencing = Query::select();
    referencing
        .expr_as(Expr::col((ConstraintRow, PgConstraint::Conrelid)), Relation)
        .from_as(PgConstraint::Table, ConstraintRow)
        .join(
            JoinType::InnerJoin,
            Reached,
            Expr::col((ConstraintRow, PgConstraint::Confrelid)).equals((Reached, Relation)),
        )
        .and_where(Expr::col((ConstraintRow, PgConstraint::Contype)).eq(text_literal("f")));
    roots.union(UnionType::Distinct, referencing);
    let mut branch_tables = Query::select();
    branch_tables
        .expr_as(Expr::col((RelationRow, PgClass::Oid)), Relation)
        .from_as(PgClass::Table, RelationRow)
        .join_as(
            JoinType::InnerJoin,
            PgNamespace::Table,
            SchemaRow,
            Expr::col((SchemaRow, PgNamespace::Oid)).equals((RelationRow, PgClass::Relnamespace)),
        )
        .and_where(Expr::col((SchemaRow, PgNamespace::Nspname)).like(BRANCH_SCHEMA_PATTERN))
        .and_where(Expr::col((RelationRow, PgClass::Relkind)).is_in([text_literal("r"), text_literal("p")]));
    let mut every_relation = Query::select();
    every_relation.column(Relation).from(Reached);
    every_relation.union(UnionType::Distinct, Query::select().column(Relation).from(BranchTables).to_owned());
    let mut with = WithClause::new();
    with.recursive(true)
        .cte(CommonTableExpression::new().table_name(Reached).columns([Relation]).query(roots).to_owned())
        .cte(
            CommonTableExpression::new()
                .table_name(BranchTables)
                .columns([Relation])
                .query(branch_tables)
                .to_owned(),
        );
    let mut locked = Query::select();
    locked
        .column((SchemaRow, PgNamespace::Nspname))
        .column((RelationRow, PgClass::Relname))
        .from_as(PgClass::Table, RelationRow)
        .join_as(
            JoinType::InnerJoin,
            PgNamespace::Table,
            SchemaRow,
            Expr::col((SchemaRow, PgNamespace::Oid)).equals((RelationRow, PgClass::Relnamespace)),
        )
        .and_where(Expr::col((RelationRow, PgClass::Oid)).in_subquery(every_relation));
    locked.with(with)
}

/// The names of the Snapshot branch schemas the tenant database holds.
fn branch_schemas_statement() -> SelectStatement {
    Query::select()
        .column(PgNamespace::Nspname)
        .from(PgNamespace::Table)
        .and_where(Expr::col(PgNamespace::Nspname).like(BRANCH_SCHEMA_PATTERN))
        .to_owned()
}

/// Reset every mutable tenant table while preserving the cached `attribute_names` and `entity_type_names` dictionaries, the `schema_metadata` contract row, the seeded unit-code catalogue, and the `context_source_change_clock` singleton.
///
/// # Errors
///
/// Returns [`DatabaseResetError::Isolation`] when the exclusive lease no longer holds its connection, and [`DatabaseResetError::Sqlx`] when the reset fails for any reason other than a busy table, or when a table stays busy past [`RESET_BUSY_DEADLINE`].
pub async fn reset_tenant(lease: &mut SerializedLease) -> Result<(), DatabaseResetError> {
    let busy = BusyTables::starting_now();
    loop {
        let connection = lease.connection()?;
        match reset_in_one_transaction(connection).await {
            Ok(()) => return Ok(()),
            Err(error) => busy.wait_out(error).await?,
        }
    }
}

/// Reset a tenant database the calling suite created and owns outright.
///
/// A suite that creates its own tenant database holds no advisory lock over it,
/// because no other suite ever connects to it. The reset therefore runs on the
/// suite's own pool instead of the exclusive lease the shared test database
/// requires, and preserves exactly the same migration-owned relations.
///
/// # Errors
///
/// Returns [`DatabaseResetError::Sqlx`] when the reset fails for any reason other than a busy table, or when a table stays busy past [`RESET_BUSY_DEADLINE`].
pub async fn reset_owned_tenant(pool: &PgPool) -> Result<(), DatabaseResetError> {
    let busy = BusyTables::starting_now();
    loop {
        let mut connection = pool.acquire().await.map_err(DatabaseResetError::from_sqlx)?;
        match reset_in_one_transaction(&mut connection).await {
            Ok(()) => return Ok(()),
            Err(error) => busy.wait_out(error).await?,
        }
    }
}

/// Lock everything the reset touches without waiting, then truncate the reset
/// relations and drop every Snapshot branch schema, all in one transaction.
async fn reset_in_one_transaction(connection: &mut PgConnection) -> Result<(), sqlx::Error> {
    let mut transaction = connection.begin().await?;
    let locked = lock_set_statement()
        .to_query()
        .fetch_all(&mut *transaction)
        .await?
        .iter()
        .map(|row| {
            Ok(TableName::from((
                Alias::new(row.try_get::<String, _>(0)?),
                Alias::new(row.try_get::<String, _>(1)?),
            )))
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?;
    RelationLock::exclusive_nowait(locked).to_query().execute(&mut *transaction).await?;
    RelationTruncation::new(reset_relations().into_iter().map(TableName::from).collect())
        .to_query()
        .execute(&mut *transaction)
        .await?;
    let branches: Vec<String> = branch_schemas_statement().to_query_scalar().fetch_all(&mut *transaction).await?;
    for branch in branches {
        SchemaDefinition::drop_cascade(Alias::new(branch)).to_query().execute(&mut *transaction).await?;
    }
    transaction.commit().await
}

/// How long a reset keeps retrying while a broker transaction holds one of its
/// tables. The brokers' background transactions (expiry sweeps, notification
/// drains) are short, so a free moment comes far sooner than this.
const RESET_BUSY_DEADLINE: Duration = Duration::from_secs(30);

/// Pause before a reset that found a table busy is run again.
const RESET_BUSY_BACKOFF: Duration = Duration::from_millis(20);

/// The retry policy of one reset: the reset takes every lock it needs at once
/// and without waiting, so a broker transaction still open on a table fails it
/// before it has changed or locked anything, and it is simply run again.
///
/// Only that failure is retried. A deadlock cannot involve a reset that never
/// waits for a lock while holding one, so it is reported rather than retried,
/// as is every other failure.
struct BusyTables {
    /// When the reset stops waiting for its tables to become free.
    deadline: Instant,
}

impl BusyTables {
    /// A retry policy whose deadline runs from now.
    fn starting_now() -> BusyTables {
        BusyTables {
            deadline: Instant::now() + RESET_BUSY_DEADLINE,
        }
    }

    /// Wait out `error` when it is a busy table (the reset's `LOCK TABLE ...
    /// NOWAIT` finding a table held) and the deadline has not passed, so the
    /// caller runs the reset again; otherwise return it.
    async fn wait_out(&self, error: sqlx::Error) -> Result<(), DatabaseResetError> {
        if is_lock_not_available(&error) && Instant::now() < self.deadline {
            sleep(RESET_BUSY_BACKOFF).await;
            return Ok(());
        }
        Err(DatabaseResetError::from_sqlx(error))
    }
}

/// `DELETE FROM table WHERE column LIKE like`.
fn delete_like(table: impl IntoIden, column: impl IntoIden, like: &str) -> DeleteStatement {
    Query::delete().from_table(table).and_where(Expr::col(column).like(like)).to_owned()
}

/// `DELETE FROM table WHERE column IN (subquery)`.
fn delete_in(table: impl IntoIden, column: impl IntoIden, subquery: SelectStatement) -> DeleteStatement {
    Query::delete().from_table(table).and_where(Expr::col(column).in_subquery(subquery)).to_owned()
}

/// Every prefix-scoped delete a cleanup runs, in dependency order.
///
/// Temporal Evolution is reached through `temporal_entities`, not through
/// `entities`: no foreign key makes deleting an entity cascade into its
/// history, so an entity delete reclaims nothing temporal.
/// `temporal_attribute_streams` cascades from `temporal_entities`, while
/// `attribute_history` and `scope_history` carry no foreign key at all
/// (hypertables cannot), so each is deleted through the streams and the
/// catalogue that name it, before the rows those names live on are removed.
///
/// `attribute_history_instance_claims` is absent deliberately: it is an
/// ordinary table whose foreign key cascades from `temporal_entities`, so the
/// catalogue delete below reclaims every claim the prefix staked. The global
/// reset still names it, because a `TRUNCATE` list is not a cascade.
fn prefix_deletes(like: &str) -> Vec<DeleteStatement> {
    let queued_writes = Query::select()
        .column(TemporalAppendIntents::WriteId)
        .from(TemporalAppendIntents::Table)
        .and_where(Expr::col(TemporalAppendIntents::EntityId).like(like))
        .to_owned();
    let streams_of_prefix = Query::select()
        .column((TemporalAttributeStreams::Table, TemporalAttributeStreams::StreamKey))
        .from(TemporalAttributeStreams::Table)
        .join(
            JoinType::InnerJoin,
            TemporalEntities::Table,
            Expr::col((TemporalEntities::Table, TemporalEntities::TemporalEntityKey))
                .equals((TemporalAttributeStreams::Table, TemporalAttributeStreams::TemporalEntityKey)),
        )
        .and_where(Expr::col((TemporalEntities::Table, TemporalEntities::Id)).like(like))
        .to_owned();
    let entities_of_prefix = Query::select()
        .column(TemporalEntities::TemporalEntityKey)
        .from(TemporalEntities::Table)
        .and_where(Expr::col(TemporalEntities::Id).like(like))
        .to_owned();
    vec![
        delete_in(WriteRecords::Table, WriteRecords::WriteId, queued_writes),
        delete_like(TemporalAppendIntents::Table, TemporalAppendIntents::EntityId, like),
        // `notification_events` carries the Entity id as text and has no foreign key
        // to `subscriptions` (a subscription delete cascades its deliveries but
        // never the events), so it is reclaimed here by the Entity-id prefix rather
        // than through any cascade.
        delete_like(NotificationEvents::Table, NotificationEvents::EntityId, like),
        delete_like(Subscriptions::Table, Subscriptions::Id, like),
        delete_like(Csr::Table, Csr::Id, like),
        delete_like(Snapshots::Table, Snapshots::Id, like),
        delete_like(Entities::Table, Entities::Id, like),
        delete_in(AttributeHistory::Table, AttributeHistory::StreamKey, streams_of_prefix),
        delete_in(ScopeHistory::Table, ScopeHistory::TemporalEntityKey, entities_of_prefix),
        delete_like(TemporalEntities::Table, TemporalEntities::Id, like),
    ]
}

/// Delete every row discoverably owned by one isolated identifier prefix.
///
/// The Entities go by a raw delete the broker's discovery census never sees,
/// so the live census is rebuilt from the search rows that remain, in the same
/// transaction (see [`rebuild_live_census`]).
///
/// # Errors
///
/// Returns [`DatabaseResetError::Sqlx`] when the transaction cannot be opened, when any prefix-scoped delete fails, or when the commit fails, and [`DatabaseResetError::Census`] when the census cannot be rebuilt.
pub async fn cleanup_prefix(pool: &PgPool, identifiers: &TestIdentifiers) -> Result<(), DatabaseResetError> {
    let like = format!("{}%", identifiers.prefix());
    let mut transaction = pool.begin().await.map_err(DatabaseResetError::from_sqlx)?;
    for statement in prefix_deletes(&like) {
        statement.to_query().execute(&mut *transaction).await.map_err(DatabaseResetError::from_sqlx)?;
    }
    rebuild_live_census(&mut transaction).await?;
    transaction.commit().await.map_err(DatabaseResetError::from_sqlx)?;
    Ok(())
}

/// Global reset or isolated cleanup failure.
#[derive(Debug, Error)]
pub enum DatabaseResetError {
    /// The serialized-state capability was unavailable.
    #[error(transparent)]
    Isolation(#[from] DatabaseIsolationError),
    /// `SQLx` could not execute the reset or cleanup transaction.
    ///
    /// Boxed for the same reason as [`DatabaseIsolationError::Sqlx`]: an
    /// `sqlx::Error` is 56 bytes, and this enum is the `Err` of every reset and
    /// prefix cleanup the harness runs.
    #[error("test database reset failed: {0}")]
    Sqlx(#[source] Box<sqlx::Error>),
    /// The discovery census could not be rebuilt after the cleanup.
    #[error("the discovery census could not be rebuilt: {0}")]
    Census(#[from] CensusError),
}

impl DatabaseResetError {
    /// Record an `SQLx` failure, moving it behind the variant's box.
    #[must_use]
    pub fn from_sqlx(error: sqlx::Error) -> DatabaseResetError {
        DatabaseResetError::Sqlx(Box::new(error))
    }
}

#[cfg(test)]
mod tests {
    use crate::database_reset::{branch_schemas_statement, lock_set_statement, prefix_deletes, reset_relations};
    use sea_query::PostgresQueryBuilder;

    /// The names of the relations the reset truncates.
    fn truncated_relations() -> Vec<String> {
        reset_relations().iter().map(ToString::to_string).collect()
    }

    /// The reset locks what the truncation reaches through a foreign key as
    /// well as the reset relations and every Snapshot branch table, so none of
    /// them is first locked by the truncation itself.
    #[test]
    fn the_lock_set_covers_what_the_truncation_cascades_to_and_every_branch_table() {
        let sql = lock_set_statement().to_string(PostgresQueryBuilder);
        assert!(sql.starts_with(r#"WITH RECURSIVE "reached" ("relation") AS"#), "{sql}");
        assert!(sql.contains(r#""k"."contype" = ('f')"#), "{sql}");
        assert!(sql.contains(r#""k"."confrelid" = "reached"."relation""#), "{sql}");
        assert!(sql.contains(r"LIKE E'snap\\_%'"), "{sql}");
        assert!(sql.contains(r#""c"."relkind" IN ('r', 'p')"#), "{sql}");
    }

    #[test]
    fn the_reset_covers_mutable_tables_and_preserves_cached_dictionaries() {
        let relations = truncated_relations();
        for table in [
            "entities",
            "entity_search",
            "entity_type_members",
            "jsonld_contexts",
            "temporal_entities",
            "temporal_entity_type_members",
            "temporal_attribute_streams",
            "temporal_stream_shapes",
            "attribute_history",
            "attribute_history_instance_claims",
            "subscriptions",
            "subscription_delivery_state",
            "context_source_registrations",
            "registration_delivery_state",
            "registration_information",
            "subscription_companions",
            "forwarded_subscriptions",
            "snapshots",
            "snapshot_preparation",
            "snapshot_notification_outbox",
            "scope_history",
            "notification_deliveries",
            "notification_events",
            "current_attributes",
            "pending_side_effects",
            "temporal_append_intents",
            "write_records",
            "entity_maps",
            "entity_map_entries",
            "entity_map_links",
        ] {
            assert!(relations.iter().any(|relation| relation == table), "the reset must cover {table}");
        }
        assert!(
            !relations
                .iter()
                .any(|relation| relation == "attribute_names" || relation == "entity_type_names")
        );
    }

    /// Temporal Evolution is detached from current entity state, so
    /// prefix cleanup has to name every temporal relation itself; relying on a
    /// cascade from `entities` would leak history across runs.
    #[test]
    fn prefix_cleanup_reaches_every_relation_the_reset_truncates() {
        let deletes = prefix_deletes("p%")
            .iter()
            .map(|statement| statement.to_string(PostgresQueryBuilder))
            .collect::<Vec<_>>()
            .join(" ");
        for relation in [
            "notification_events",
            "subscriptions",
            "context_source_registrations",
            "snapshots",
            "entities",
            "attribute_history",
            "scope_history",
            "temporal_entities",
        ] {
            assert!(
                deletes.contains(&format!(r#"DELETE FROM "{relation}""#)),
                "prefix cleanup must delete from {relation}"
            );
        }
    }

    /// `temporal_entities` is deleted last so the statements that locate
    /// history rows through it still find their catalogue rows.
    #[test]
    fn prefix_cleanup_removes_the_temporal_catalogue_after_the_rows_that_name_it() {
        let deletes = prefix_deletes("p%")
            .iter()
            .map(|statement| statement.to_string(PostgresQueryBuilder))
            .collect::<Vec<_>>();
        let position = |prefix: &str| {
            deletes
                .iter()
                .position(|statement| statement.starts_with(prefix))
                .unwrap_or_else(|| panic!("{prefix} is deleted"))
        };
        let catalogue = position(r#"DELETE FROM "temporal_entities""#);
        for relation in [r#"DELETE FROM "attribute_history""#, r#"DELETE FROM "scope_history""#] {
            assert!(position(relation) < catalogue, "{relation} must run before the catalogue it reads");
        }
    }

    #[test]
    fn the_reset_drops_every_snapshot_branch_schema_and_keeps_the_live_default_schema() {
        let sql = branch_schemas_statement().to_string(PostgresQueryBuilder);
        assert_eq!(sql, r#"SELECT "nspname" FROM "pg_namespace" WHERE "nspname" LIKE E'snap\\_%'"#);
        assert!(!truncated_relations().iter().any(|relation| relation == "context_spaces"));
    }

    /// Truncating `schema_metadata` makes the tenant read as never-stamped, and
    /// the startup verification refuses every connection after it. Every
    /// registration write advances the one seeded `context_source_change_clock`
    /// row; an emptied clock leaves those writes nothing to advance.
    #[test]
    fn the_reset_leaves_the_schema_contract_and_change_clock_singletons_in_place() {
        let relations = truncated_relations();
        assert!(!relations.iter().any(|relation| relation == "schema_metadata"));
        assert!(!relations.iter().any(|relation| relation == "context_source_change_clock"));
    }
}
