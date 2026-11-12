//! The start of every test: a database at the v1.0.0 schema holding the
//! dictionary entries the seeded rows refer to.

use crate::{
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

/// A database at the v1.0.0 schema, with the dictionary entries every test
/// starts from.
pub(crate) async fn at_v1_0_0() -> Result<(UpgradeDatabase, Keys), UpgradeTestError> {
    let server = UpgradeServer::from_environment()?;
    let database = UpgradeDatabase::create(&server).await?;
    database.migrate_to_v1_0_0().await?;
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
