//! Whether the attached `TimescaleDB` edition provides the hypercore columnstore.
//!
//! Hypercore -- `ALTER TABLE ... SET (timescaledb.enable_columnstore, ...)` and
//! `add_columnstore_policy` -- is a TSL-licensed capability. The Apache-2.0 core
//! ships neither the DDL nor the procedure, so the runtime reconciler gates
//! every columnstore statement on this probe and runs the temporal hypertables
//! as plain hypertables where it is absent: it never issues a statement the
//! edition cannot answer. No migration touches the columnstore at all.
//!
//! This is a different question from the license marker
//! `andromeda-postgresql-context-store` reports for the info endpoint: that reads
//! the `timescaledb.license` setting to describe the build, whereas this probes
//! one procedure's presence to decide whether a columnstore statement may run.

use andromeda_postgresql_seaquery::introspection::function_exists;
use sea_query::{Alias, PostgresQueryBuilder, Query, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::{AssertSqlSafe, PgPool};

/// The `TimescaleDB` procedure whose presence marks a hypercore-capable edition.
const COLUMNSTORE_POLICY_PROC: &str = "add_columnstore_policy";

/// Whether the attached `TimescaleDB` edition provides the hypercore columnstore.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ColumnstoreSupport {
    /// The columnstore DDL and policy procedure are registered: the TSL edition.
    Present,
    /// Neither is registered: the Apache-2.0 core, which runs the hypertables
    /// without compression.
    Absent,
}

impl ColumnstoreSupport {
    /// Probe the database for the hypercore columnstore procedure.
    ///
    /// Detected once when a tenant is opened and carried on the repository, since
    /// the edition cannot change without the server restarting.
    ///
    /// # Errors
    ///
    /// Returns [`sqlx::Error`] when the probe cannot be executed against the pool.
    pub async fn detect(pool: &PgPool) -> Result<ColumnstoreSupport, sqlx::Error> {
        let (sql, values) = detect_statement().build_sqlx(PostgresQueryBuilder);
        let present: bool = sqlx::query_scalar_with(AssertSqlSafe(sql), values).fetch_one(pool).await?;
        Ok(if present { ColumnstoreSupport::Present } else { ColumnstoreSupport::Absent })
    }
}

/// The probe reading whether `add_columnstore_policy` is registered in `pg_proc`.
fn detect_statement() -> SelectStatement {
    Query::select()
        .expr_as(function_exists(COLUMNSTORE_POLICY_PROC), Alias::new("present"))
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::columnstore_capability::detect_statement;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn the_probe_asks_pg_proc_for_the_columnstore_policy_procedure() {
        let sql = detect_statement().to_string(PostgresQueryBuilder);
        assert!(
            sql.contains("EXISTS(SELECT 1 FROM \"pg_proc\" WHERE \"proname\" = 'add_columnstore_policy')"),
            "{sql}"
        );
        assert!(sql.contains("AS \"present\""), "{sql}");
    }
}
