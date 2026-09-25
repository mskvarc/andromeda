//! The platform migration set.

use sqlx::migrate::{Migration, Migrator};
use std::borrow::Cow;

/// The ledger the platform migration set is recorded in.
pub const PLATFORM_LEDGER: &str = "_andromeda_platform_migrations";

/// The platform migrations, exactly as `migrations/platform/` holds them.
static PLATFORM_MIGRATIONS: Migrator = sqlx::migrate!("../../migrations/platform");

/// The platform migrations, in version order.
#[must_use]
pub fn platform_migrations() -> &'static [Migration] {
    &PLATFORM_MIGRATIONS.migrations[..]
}

/// The migrator for the platform database, recorded in [`PLATFORM_LEDGER`].
#[must_use]
pub fn platform_migrator() -> Migrator {
    let mut migrator = Migrator {
        migrations: Cow::Borrowed(platform_migrations()),
        ..Migrator::DEFAULT
    };
    migrator.dangerous_set_table_name(PLATFORM_LEDGER);
    migrator
}

#[cfg(test)]
mod tests {
    use crate::migration_set::platform::{PLATFORM_LEDGER, platform_migrator};
    use pretty_assertions::assert_eq;

    #[test]
    fn the_platform_migrator_records_into_the_platform_ledger() {
        let migrator = platform_migrator();
        assert_eq!(migrator.table_name, PLATFORM_LEDGER);
        assert_eq!(PLATFORM_LEDGER, "_andromeda_platform_migrations");
    }

    #[test]
    fn the_platform_migrator_never_ignores_a_missing_migration() {
        let migrator = platform_migrator();
        assert!(!migrator.ignore_missing);
        assert!(migrator.locking);
    }
}
