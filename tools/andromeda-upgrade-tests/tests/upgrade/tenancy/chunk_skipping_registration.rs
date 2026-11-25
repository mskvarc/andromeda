//! The columns history skips chunks on, registered when the broker opens a
//! tenant: each one exactly once, whether two opens of the tenant run at once
//! or v1.0.0 left a column registered twice, so a history write that needs a
//! new chunk lands.

use crate::fixtures::{
    current_state::{insert_entity, intern_attribute},
    history::{InstanceIdentity, insert_history_row, insert_stream},
    registry::open_tenant_pool,
    test_error::UpgradeTestError,
};
use andromeda_configuration::schema::database::{name_cache::NameCacheConfig, write_reserve::WriteReserve};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use andromeda_postgresql_name_dictionary::name_cache::NameCache;
use andromeda_postgresql_pool::pool_share::PoolShare;
use andromeda_postgresql_seaquery::{
    chunk_skipping::{SkippingColumn, chunk_skipping_columns_statement, enable_chunk_skipping},
    command::Command,
    database_definition::DatabaseDefinition,
    setting::Setting,
    statement::Statement,
};
use andromeda_postgresql_temporal_store::{columnstore_capability::ColumnstoreSupport, hypertable_policy::HypertableName, repo::TemporalRepository};
use andromeda_temporal::storage_policy::TemporalStoragePolicy;
use andromeda_upgrade_tests::{upgrade_database::UpgradeDatabase, upgrade_server::UpgradeServer};
use pretty_assertions::assert_eq;
use sea_query::{Alias, Iden, Query};
use sqlx::{Error, PgExecutor, PgPool, Row};
use std::{num::NonZeroU32, sync::Arc};

/// The storage policy a tenant opens under: a columnstore lag of seven days,
/// no chunk interval or retention of the operator's.
fn policy() -> TemporalStoragePolicy {
    TemporalStoragePolicy {
        chunk_interval_days: None,
        columnstore_after_days: NonZeroU32::MIN.saturating_add(6),
        retention_after_days: None,
    }
}

/// The columns history declares for chunk skipping, in declaration order.
fn declared() -> Vec<String> {
    HypertableName::AttributeHistory
        .chunk_skipping_columns()
        .iter()
        .map(ToString::to_string)
        .collect()
}

/// Installs the default every tenant database carries, without which no
/// session records a chunk's range or registers a column.
async fn record_ranges_by_default(database: &UpgradeDatabase) -> Result<(), UpgradeTestError> {
    DatabaseDefinition::set_default(Alias::new(database.database_name().as_str()), Setting::TimescaledbEnableChunkSkipping, "on")
        .to_query()
        .execute(database.pool())
        .await?;
    Ok(())
}

/// The temporal store a tenant opened on `pool` reconciles history through.
async fn temporal_store(pool: &PgPool) -> Result<TemporalRepository, UpgradeTestError> {
    let columnstore = ColumnstoreSupport::detect(pool).await?;
    Ok(TemporalRepository::new(
        pool,
        PoolShare::of_reads(pool, WriteReserve::DEFAULT),
        Arc::new(NameCache::<AttributeName>::new(pool, &NameCacheConfig::default())),
        Arc::new(NameCache::<EntityTypeName>::new(pool, &NameCacheConfig::default())),
        columnstore,
    ))
}

/// Registers `column` of history for chunk skipping, as a release opening the
/// tenant did.
async fn register<'e>(executor: impl PgExecutor<'e>, column: &str) -> Result<(), UpgradeTestError> {
    Query::select()
        .expr(enable_chunk_skipping(HypertableName::AttributeHistory.into(), column))
        .to_owned()
        .to_query()
        .execute(executor)
        .await?;
    Ok(())
}

/// The registrations of history's chunk-skipping columns, one name per
/// registration, sorted.
async fn registrations(pool: &PgPool) -> Result<Vec<String>, UpgradeTestError> {
    let mut names: Vec<String> = chunk_skipping_columns_statement(HypertableName::AttributeHistory.into())
        .to_query()
        .fetch_all(pool)
        .await?
        .iter()
        .map(|row| row.try_get(SkippingColumn.unquoted()))
        .collect::<Result<_, _>>()?;
    names.sort();
    Ok(names)
}

/// Adds an Entity and the stream of its one attribute, and returns the
/// stream's key.
async fn seed_stream(pool: &PgPool) -> Result<i64, UpgradeTestError> {
    let attribute_key = intern_attribute(pool, "https://example.org/temperature").await?;
    let entity_key = insert_entity(pool, "urn:ngsi-ld:Sensor:1").await?;
    insert_stream(pool, entity_key, attribute_key).await
}

/// Writes one history row on `stream_key`, which creates history's first
/// chunk.
async fn write_history(pool: &PgPool, stream_key: i64) -> Result<(), UpgradeTestError> {
    insert_history_row(pool, stream_key, &InstanceIdentity::Generated("00000000-0000-4000-8000-0000000000c1"), &[]).await
}

/// The unique key of the catalogue's chunk ranges, one row per hypertable,
/// chunk and registered column.
const CHUNK_RANGE_KEY: &str = "chunk_column_stats_ht_id_chunk_id_colname_key";

/// The declared columns, sorted, each registered once.
fn each_declared_once() -> Vec<String> {
    let mut names = declared();
    names.sort();
    names
}

#[tokio::test]
async fn two_opens_of_a_tenant_at_once_register_each_column_once_and_history_creates_its_first_chunk() -> Result<(), UpgradeTestError> {
    let server = UpgradeServer::from_environment()?;
    let mut database = UpgradeDatabase::create(&server).await?;
    database.migrate_to_current().await?;
    record_ranges_by_default(&database).await?;
    let first = open_tenant_pool(&server, database.database_name()).await?;
    let second = open_tenant_pool(&server, database.database_name()).await?;
    let (first_store, second_store) = (temporal_store(&first).await?, temporal_store(&second).await?);
    let policy = policy();
    let (first_outcome, second_outcome) = tokio::join!(first_store.reconcile_storage_policy(&policy), second_store.reconcile_storage_policy(&policy));
    first_outcome?;
    second_outcome?;
    assert_eq!(registrations(&first).await?, each_declared_once());
    let stream_key = seed_stream(&first).await?;
    write_history(&first, stream_key).await?;
    first.close().await;
    second.close().await;
    database.drop_database().await?;
    Ok(())
}

#[tokio::test]
async fn a_column_v1_0_0_registered_twice_is_registered_once_when_the_tenant_opens_and_history_creates_chunks_again() -> Result<(), UpgradeTestError> {
    let server = UpgradeServer::from_environment()?;
    let mut database = UpgradeDatabase::create(&server).await?;
    database.migrate_to_v1_0_0().await?;
    record_ranges_by_default(&database).await?;
    let first = open_tenant_pool(&server, database.database_name()).await?;
    let second = open_tenant_pool(&server, database.database_name()).await?;
    // Each release open looks for an earlier registration among committed rows
    // only, so the second registration lands while the first is uncommitted.
    let mut held = first.begin().await?;
    register(&mut *held, "created_at").await?;
    register(&second, "created_at").await?;
    held.commit().await?;
    assert_eq!(registrations(&second).await?, vec!["created_at".to_owned(), "created_at".to_owned()]);
    database.migrate_to_current().await?;
    let stream_key = seed_stream(&second).await?;
    // The new chunk's range row for the twice-registered column is inserted
    // twice, once for each registration.
    let refused = write_history(&second, stream_key).await;
    assert!(
        matches!(&refused, Err(UpgradeTestError::Query(Error::Database(error))) if error.constraint() == Some(CHUNK_RANGE_KEY)),
        "{refused:?}"
    );
    temporal_store(&second).await?.reconcile_storage_policy(&policy()).await?;
    assert_eq!(registrations(&second).await?, each_declared_once());
    write_history(&second, stream_key).await?;
    first.close().await;
    second.close().await;
    database.drop_database().await?;
    Ok(())
}
