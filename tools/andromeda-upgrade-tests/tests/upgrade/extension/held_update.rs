//! A tenant database whose `TimescaleDB` stands at the schema contract's floor
//! while another session holds a lock the update needs. The update replaces
//! the `timescaledb_information` views, so a transaction that read one of them
//! holds it back.
//!
//! An update held back past every retry fails the open with the refused lock
//! and leaves the installed version, and the next open updates it. An update
//! that waits long enough for the database's idle-in-transaction timeout to
//! pass still completes, since the session holding the update lock idles while
//! the update runs. An update abandoned while it waits leaves the installed
//! version for the next open.

use crate::fixtures::test_error::UpgradeTestError;
use andromeda_postgresql_context_store::{
    error::ContextStoreError,
    extension_update::update_extensions,
    schema_contract::server_requirements::REQUIRED_TIMESCALEDB_VERSION,
    tenant_migration::{TenantSchemaState, tenant_schema_state},
};
use andromeda_postgresql_pool::schema_change_lock::is_lock_not_available;
use andromeda_postgresql_seaquery::{
    aggregate::count_all,
    command::Command,
    introspection::extension_version,
    statement::Statement,
    test_extension::ExtensionCreationAtVersion,
};
use andromeda_upgrade_tests::{upgrade_database::UpgradeDatabase, upgrade_server::UpgradeServer};
use pretty_assertions::{assert_eq, assert_ne};
use sea_query::{Alias, Query};
use sqlx::{PgPool, Postgres, Transaction};
use std::time::Duration;
use tokio::time::{sleep, timeout};
use url::Url;

/// The `TimescaleDB` version `pool`'s database holds.
async fn timescaledb_version(pool: &PgPool) -> Result<Option<String>, UpgradeTestError> {
    let select = Query::select().expr(extension_version("timescaledb")).to_owned();
    Ok(select.to_query_scalar::<Option<String>>().fetch_one(pool).await?)
}

/// A migrated database holding `TimescaleDB` at the floor, and the URL the
/// broker's update reaches it at.
async fn database_at_floor(server: &UpgradeServer) -> Result<(UpgradeDatabase, Url), UpgradeTestError> {
    let mut database = UpgradeDatabase::create(server).await?;
    ExtensionCreationAtVersion::new(Alias::new("timescaledb"), &REQUIRED_TIMESCALEDB_VERSION.to_string())
        .to_query()
        .execute(database.pool())
        .await?;
    database.migrate_to_current().await?;
    let url = server.url().with_database_name(database.database_name());
    Ok((database, url))
}

/// A transaction that read `timescaledb_information.hypertables` and holds
/// the lock the update must take to replace the view until it ends.
async fn view_reader(pool: &PgPool) -> Result<Transaction<'static, Postgres>, UpgradeTestError> {
    let mut transaction = pool.begin().await?;
    Query::select()
        .expr(count_all())
        .from((Alias::new("timescaledb_information"), Alias::new("hypertables")))
        .to_owned()
        .to_query_scalar::<i64>()
        .fetch_one(&mut *transaction)
        .await?;
    Ok(transaction)
}

#[tokio::test]
async fn an_update_a_session_holds_back_past_every_retry_fails_the_open_and_the_next_open_makes_it() -> Result<(), UpgradeTestError> {
    let server = UpgradeServer::from_environment()?;
    let (database, url) = database_at_floor(&server).await?;
    let floor = REQUIRED_TIMESCALEDB_VERSION.to_string();
    let reader = view_reader(database.pool()).await?;

    let refused = update_extensions(&url).await;

    // The reader holds the database pool's one connection until it ends.
    reader.rollback().await?;
    assert!(
        matches!(&refused, Err(ContextStoreError::Database(error)) if is_lock_not_available(error)),
        "the update fails with the refused lock: {refused:?}"
    );
    assert_eq!(timescaledb_version(database.pool()).await?.as_deref(), Some(floor.as_str()));
    assert_eq!(tenant_schema_state(database.pool()).await?, TenantSchemaState::NeedsMigration);

    update_extensions(&url).await?;

    assert_ne!(timescaledb_version(database.pool()).await?.as_deref(), Some(floor.as_str()));
    assert_eq!(tenant_schema_state(database.pool()).await?, TenantSchemaState::Current);
    database.drop_database().await?;
    Ok(())
}

#[tokio::test]
async fn an_update_held_back_longer_than_the_idle_in_transaction_timeout_completes() -> Result<(), UpgradeTestError> {
    let server = UpgradeServer::from_environment()?;
    let (database, mut url) = database_at_floor(&server).await?;
    let floor = REQUIRED_TIMESCALEDB_VERSION.to_string();
    // A one-second timeout stands in for the server's ten minutes: the reader
    // holds the update back for three, so the session holding the update lock
    // idles in its transaction past the timeout before the update commits.
    url.query_pairs_mut().append_pair("options", "-c idle_in_transaction_session_timeout=1000");
    let reader = view_reader(database.pool()).await?;

    let (updated, released) = tokio::join!(update_extensions(&url), async {
        sleep(Duration::from_secs(3)).await;
        reader.rollback().await
    });

    released?;
    updated?;
    assert_ne!(timescaledb_version(database.pool()).await?.as_deref(), Some(floor.as_str()));
    assert_eq!(tenant_schema_state(database.pool()).await?, TenantSchemaState::Current);
    database.drop_database().await?;
    Ok(())
}

#[tokio::test]
async fn an_update_abandoned_while_it_waits_for_a_lock_leaves_the_installed_version_and_the_next_open_makes_it() -> Result<(), UpgradeTestError> {
    let server = UpgradeServer::from_environment()?;
    let (database, url) = database_at_floor(&server).await?;
    let floor = REQUIRED_TIMESCALEDB_VERSION.to_string();
    let reader = view_reader(database.pool()).await?;

    let abandoned = timeout(Duration::from_secs(1), update_extensions(&url)).await;
    // The abandoned update's server session waits out its own two-second lock
    // timeout and fails before the reader lets the lock go.
    sleep(Duration::from_secs(3)).await;
    reader.rollback().await?;

    assert!(abandoned.is_err(), "the update was still waiting for the lock: {abandoned:?}");
    assert_eq!(timescaledb_version(database.pool()).await?.as_deref(), Some(floor.as_str()));

    update_extensions(&url).await?;

    assert_ne!(timescaledb_version(database.pool()).await?.as_deref(), Some(floor.as_str()));
    assert_eq!(tenant_schema_state(database.pool()).await?, TenantSchemaState::Current);
    database.drop_database().await?;
    Ok(())
}
