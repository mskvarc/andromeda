//! Whether the attached `TimescaleDB` edition provides the hypercore columnstore.
//!
//! Hypercore (`ALTER TABLE ... SET (timescaledb.enable_columnstore, ...)` and
//! `convert_to_columnstore`) is a TSL-licensed capability. The runtime
//! reconciler and the conversion sweep gate every columnstore statement on
//! this probe and run the temporal hypertables as plain hypertables where it is
//! absent: they never issue a statement the edition cannot answer. No migration
//! touches the columnstore at all.
//!
//! The probe reads the `timescaledb.license` setting. The procedures
//! themselves are no evidence: a server licensed `apache` still registers
//! every columnstore procedure in `pg_proc` and refuses each call with
//! "functionality not supported under the current "apache" license".

use andromeda_postgresql_seaquery::{columnstore_chunk::columnstore_licensed, statement::Statement};
use sea_query::{Alias, Query, SelectStatement};
use sqlx::PgPool;

/// Whether the attached `TimescaleDB` edition provides the hypercore columnstore.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ColumnstoreSupport {
    /// The server runs under the Timescale License: the TSL edition.
    Present,
    /// The server runs the Apache-2.0 edition, or no `TimescaleDB` at all,
    /// and runs the hypertables without compression.
    Absent,
}

impl ColumnstoreSupport {
    /// Probe the database for the license the columnstore needs.
    ///
    /// Detected once when a tenant is opened and carried on the repository, since
    /// the license cannot change without the server restarting.
    ///
    /// # Errors
    ///
    /// Returns [`sqlx::Error`] when the probe cannot be executed against the pool.
    pub async fn detect(pool: &PgPool) -> Result<ColumnstoreSupport, sqlx::Error> {
        let licensed: bool = detect_statement().to_query_scalar().fetch_one(pool).await?;
        Ok(ColumnstoreSupport::of_license(licensed))
    }

    /// The support the license probe's answer grants.
    const fn of_license(licensed: bool) -> ColumnstoreSupport {
        if licensed { ColumnstoreSupport::Present } else { ColumnstoreSupport::Absent }
    }
}

/// The probe reading whether `timescaledb.license` names the TSL edition.
fn detect_statement() -> SelectStatement {
    Query::select().expr_as(columnstore_licensed(), Alias::new("licensed")).to_owned()
}

#[cfg(test)]
mod tests {
    use crate::columnstore_capability::{ColumnstoreSupport, detect_statement};
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn the_probe_reads_the_license_setting_without_failing_when_it_is_undefined() {
        assert_eq!(
            detect_statement().to_string(PostgresQueryBuilder),
            r#"SELECT COALESCE(current_setting('timescaledb.license', TRUE) = 'timescale', FALSE) AS "licensed""#
        );
    }

    #[test]
    fn only_the_timescale_license_provides_the_columnstore() {
        assert_eq!(ColumnstoreSupport::of_license(true), ColumnstoreSupport::Present);
        assert_eq!(ColumnstoreSupport::of_license(false), ColumnstoreSupport::Absent);
    }
}
