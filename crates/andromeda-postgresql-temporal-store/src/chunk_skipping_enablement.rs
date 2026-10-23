//! Registering the columns whose per-chunk ranges let the planner exclude a
//! converted chunk a time filter cannot match.
//!
//! History is partitioned on the write clock, which no `timeproperty` follows,
//! so chunk ranges alone exclude nothing a temporal query bounds. Chunk
//! skipping keeps a range of each registered column per chunk, computed when
//! the chunk is converted, and the planner excludes a chunk whose range a
//! bound on that column cannot meet, at plan time for a literal bound and at
//! executor startup for a parameter.
//!
//! `TimescaleDB` 2.31 records a chunk's range only in a session with
//! `timescaledb.enable_chunk_skipping` on, and a skipping plan excludes a chunk
//! whose range was never recorded: a chunk created or converted in a session
//! without the setting returns no row to a query that should match it. The
//! setting is therefore one of the defaults installed on every tenant database,
//! which every session inherits, and registering a column requires it: on a
//! database without it the registration fails rather than leave the
//! hypertable half configured. Registering a column on a hypertable that
//! already holds chunks gives each an unbounded range, which no plan excludes.
//!
//! Registered only on an edition with a columnstore, since only a conversion
//! records a range. A column registered beyond the declared ones is left
//! alone: its ranges only ever exclude chunks no row of it lies in.

use crate::{
    bounded_lock_wait::{StatementWrite, execute_with_bounded_lock_wait},
    error::TemporalStoreError,
    hypertable_policy::HypertableName,
    storage_policy::PolicyReconciliation,
};
use andromeda_postgresql_seaquery::{
    chunk_skipping::{SkippingColumn, chunk_skipping_columns_statement, enable_chunk_skipping},
    statement::Statement,
};
use sea_query::{DynIden, Iden, Query};
use sqlx::{PgPool, Row};

/// Register every column `hypertable` skips chunks on that is not yet
/// registered.
///
/// # Errors
///
/// Returns [`TemporalStoreError::Database`] when the catalogue cannot be read
/// or a registration fails, including on a database whose sessions do not
/// carry `timescaledb.enable_chunk_skipping`.
pub(crate) async fn reconcile_chunk_skipping(pool: &PgPool, hypertable: HypertableName) -> Result<PolicyReconciliation, TemporalStoreError> {
    let registered: Vec<String> = chunk_skipping_columns_statement(hypertable.into())
        .to_query()
        .fetch_all(pool)
        .await?
        .iter()
        .map(|row| row.try_get(SkippingColumn.unquoted()))
        .collect::<Result<_, _>>()?;
    let declared = hypertable.chunk_skipping_columns();
    let missing = unregistered(&declared, &registered);
    if missing.is_empty() {
        return Ok(PolicyReconciliation::Unchanged);
    }
    for column in missing {
        let registration = Query::select().expr(enable_chunk_skipping(hypertable.into(), &column.to_string())).to_owned();
        execute_with_bounded_lock_wait(pool, &StatementWrite(registration)).await?;
    }
    Ok(PolicyReconciliation::Adjusted)
}

/// The declared columns the catalogue does not list as registered, in
/// declaration order.
fn unregistered<'d>(declared: &'d [DynIden], registered: &[String]) -> Vec<&'d DynIden> {
    declared
        .iter()
        .filter(|column| !registered.iter().any(|name| *name == column.to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::{chunk_skipping_enablement::unregistered, hypertable_policy::HypertableName};
    use pretty_assertions::assert_eq;

    #[test]
    fn only_the_declared_columns_the_catalogue_lacks_are_registered_in_declaration_order() {
        let declared = HypertableName::AttributeHistory.chunk_skipping_columns();
        let registered = vec!["created_at".to_owned(), "value_number".to_owned()];
        assert_eq!(
            unregistered(&declared, &registered).iter().map(ToString::to_string).collect::<Vec<_>>(),
            vec!["observed_at", "modified_at"]
        );
    }

    #[test]
    fn nothing_is_registered_once_every_declared_column_is() {
        let declared = HypertableName::AttributeHistory.chunk_skipping_columns();
        let registered: Vec<String> = declared.iter().map(ToString::to_string).collect();
        assert_eq!(
            unregistered(&declared, &registered).iter().map(ToString::to_string).collect::<Vec<_>>(),
            Vec::<String>::new()
        );
    }
}
