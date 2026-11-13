//! The start of every test: a database at the v1.0.0 schema holding the
//! dictionary entries the seeded rows refer to.

use crate::fixtures::{
    current_state::{intern_attribute, intern_type},
    test_error::UpgradeTestError,
    vocabulary::{SEAL_ATTRIBUTE, SEALED_TYPE},
};
use andromeda_upgrade_tests::{upgrade_database::UpgradeDatabase, upgrade_server::UpgradeServer};
use sqlx::PgPool;

/// The expanded name of the attribute the seeded Entities carry.
const TEMPERATURE: &str = "https://example.org/temperature";

/// The keys of the dictionary entries every test starts from.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Keys {
    /// The attribute a seal signs.
    pub(crate) temperature: i32,
    /// The type the seals name, which the seeded Entities carry.
    pub(crate) store: i32,
}

/// A tenant database at the v1.0.0 schema, with the dictionary entries every
/// test starts from.
pub(crate) async fn at_v1_0_0() -> Result<(UpgradeDatabase, Keys), UpgradeTestError> {
    let database = UpgradeDatabase::create(&UpgradeServer::from_environment()?).await?;
    database.migrate_to_v1_0_0().await?;
    with_keys(database).await
}

/// The platform database at the v1.0.0 schema, with the dictionary entries
/// every test starts from.
pub(crate) async fn platform_at_v1_0_0() -> Result<(UpgradeDatabase, Keys), UpgradeTestError> {
    let database = UpgradeDatabase::create(&UpgradeServer::from_environment()?).await?;
    database.migrate_platform_to_v1_0_0().await?;
    with_keys(database).await
}

/// `database` with the dictionary entries every test starts from added.
async fn with_keys(database: UpgradeDatabase) -> Result<(UpgradeDatabase, Keys), UpgradeTestError> {
    let keys = Keys {
        temperature: intern_attribute(database.pool(), TEMPERATURE).await?,
        store: intern_type(database.pool(), SEALED_TYPE).await?,
    };
    Ok((database, keys))
}

/// Adds the attribute seals are stored under to the dictionary, as the first
/// write of a seal did, and returns its key.
pub(crate) async fn intern_seal_attribute(pool: &PgPool) -> Result<i32, UpgradeTestError> {
    intern_attribute(pool, SEAL_ATTRIBUTE).await
}
