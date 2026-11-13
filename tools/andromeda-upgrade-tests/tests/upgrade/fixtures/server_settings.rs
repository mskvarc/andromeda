//! Settings an operator may have changed on the server a tenant upgrades on.

use crate::fixtures::test_error::UpgradeTestError;
use andromeda_postgresql_seaquery::{
    command::Command,
    setting::{Setting, SettingAssignment, SettingScope},
};
use sqlx::PgPool;

/// Makes the session read a backslash in a plain string constant as an
/// escape, as a server with `standard_conforming_strings` off does. The
/// session is the one connection of the rehearsal's pool, which the
/// migrations run on.
pub(crate) async fn read_backslash_escapes_in_plain_strings(pool: &PgPool) -> Result<(), UpgradeTestError> {
    SettingAssignment::new(Setting::StandardConformingStrings, "off", SettingScope::Session)
        .to_query()
        .execute(pool)
        .await?;
    Ok(())
}
