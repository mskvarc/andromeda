//! `ALTER EXTENSION ... UPDATE`, and the setting that keeps `TimescaleDB`
//! unloaded in the reads around it.
//!
//! Neither statement is one `SeaQuery` models. The update names its extension
//! as an identifier, and the setting must be a `SET` statement: the
//! `TimescaleDB` loader loads the extension library while it analyses any other
//! statement, a `SELECT set_config(...)` included, before that statement could
//! turn loading off.

use crate::schema_contract::versioned_extension::VersionedExtension;

/// `SET LOCAL timescaledb.disable_load = on`: keep the `TimescaleDB` library
/// unloaded in this connection until the transaction ends.
///
/// A session that has loaded one `TimescaleDB` version is closed by the
/// extension at its next statement once another session updates it, so a read
/// that decides whether an update is due runs under this setting and leaves
/// its connection usable after the update. `LOCAL` scopes it to one
/// transaction, which a transaction-pooling proxy keeps on one backend and
/// which hands the backend back with loading restored.
pub const DEFER_TIMESCALEDB_LOAD: &str = "SET LOCAL timescaledb.disable_load = on";

/// `ALTER EXTENSION <name> UPDATE`: update one extension to the version the
/// server's installation ships.
///
/// `TimescaleDB` accepts it only as the first statement of a session, before
/// the old library is loaded.
#[must_use]
pub const fn update_extension(extension: VersionedExtension) -> &'static str {
    match extension {
        VersionedExtension::PostGis => "ALTER EXTENSION postgis UPDATE",
        VersionedExtension::TimescaleDb => "ALTER EXTENSION timescaledb UPDATE",
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        schema_contract::versioned_extension::VersionedExtension,
        schema_ddl::extension::{DEFER_TIMESCALEDB_LOAD, update_extension},
    };
    use pretty_assertions::assert_eq;
    use strum::IntoEnumIterator;

    #[test]
    fn every_versioned_extension_is_updated_by_its_catalogue_name() {
        for extension in VersionedExtension::iter() {
            assert_eq!(update_extension(extension), format!("ALTER EXTENSION {} UPDATE", extension.catalog_name()));
        }
    }

    #[test]
    fn the_load_deferral_is_scoped_to_the_transaction() {
        assert_eq!(DEFER_TIMESCALEDB_LOAD, "SET LOCAL timescaledb.disable_load = on");
    }
}
