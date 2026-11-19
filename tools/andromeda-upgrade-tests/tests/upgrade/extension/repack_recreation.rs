//! A tenant database whose `pg_repack` stands below the version the server
//! ships, which no update script reaches, so the extension reconciliation
//! drops it and creates it again. While an object outside the extension
//! depends on its members, as a repack run's leftovers do, the drop is refused
//! and the installed version stays until a later open finds them gone.

use crate::fixtures::test_error::UpgradeTestError;
use andromeda_postgresql_context_store::extension_update::update_extensions;
use andromeda_postgresql_seaquery::{
    command::Command,
    introspection::{extension_shipped_version, extension_version},
    statement::Statement,
    test_extension::{InstalledVersionRecord, MemberView, MemberViewRemoval},
};
use andromeda_upgrade_tests::{upgrade_database::UpgradeDatabase, upgrade_server::UpgradeServer};
use pretty_assertions::assert_eq;
use sea_query::{Alias, Query};
use sqlx::PgPool;
use url::Url;

/// The version below the shipped 1.5.3 the database records.
const BELOW_SHIPPED: &str = "1.5.2";

/// The `pg_repack` version `pool`'s database holds, and the one the server
/// ships.
async fn repack_versions(pool: &PgPool) -> Result<(Option<String>, Option<String>), UpgradeTestError> {
    let select = Query::select()
        .expr(extension_version("pg_repack"))
        .expr(extension_shipped_version("pg_repack"))
        .to_owned();
    Ok(select.to_query_as::<(Option<String>, Option<String>)>().fetch_one(pool).await?)
}

/// A migrated database whose reconciliation installed `pg_repack` at the
/// shipped version and which then records it below, and its URL.
async fn database_below_shipped(server: &UpgradeServer) -> Result<(UpgradeDatabase, Url), UpgradeTestError> {
    let mut database = UpgradeDatabase::create(server).await?;
    database.migrate_to_current().await?;
    let url = server.url().with_database_name(database.database_name());
    update_extensions(&url).await?;
    let (installed, shipped) = repack_versions(database.pool()).await?;
    assert_eq!(installed, shipped, "the reconciliation installs the shipped version");
    InstalledVersionRecord::new(Alias::new("pg_repack"), BELOW_SHIPPED)
        .to_query()
        .execute(database.pool())
        .await?;
    Ok((database, url))
}

#[tokio::test]
async fn a_pg_repack_below_the_shipped_version_is_dropped_and_created_again_at_it() -> Result<(), UpgradeTestError> {
    let server = UpgradeServer::from_environment()?;
    let (database, url) = database_below_shipped(&server).await?;

    update_extensions(&url).await?;

    let (installed, shipped) = repack_versions(database.pool()).await?;
    assert_eq!(installed, shipped);
    database.drop_database().await?;
    Ok(())
}

#[tokio::test]
async fn a_pg_repack_whose_members_an_outside_object_holds_keeps_its_version_until_the_object_is_gone() -> Result<(), UpgradeTestError> {
    let server = UpgradeServer::from_environment()?;
    let (database, url) = database_below_shipped(&server).await?;
    MemberView::new(Alias::new("repack_watch"), Alias::new("repack"), Alias::new("tables"))
        .to_query()
        .execute(database.pool())
        .await?;

    update_extensions(&url).await?;

    assert_eq!(repack_versions(database.pool()).await?.0.as_deref(), Some(BELOW_SHIPPED));

    MemberViewRemoval::new(Alias::new("repack_watch")).to_query().execute(database.pool()).await?;
    update_extensions(&url).await?;

    let (installed, shipped) = repack_versions(database.pool()).await?;
    assert_eq!(installed, shipped);
    database.drop_database().await?;
    Ok(())
}
