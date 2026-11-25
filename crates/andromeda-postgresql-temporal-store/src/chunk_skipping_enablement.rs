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
//! A tenant's runtime can be built twice at once, by a request and by a wake,
//! and `enable_chunk_skipping` looks for an earlier registration only among
//! committed rows, so two registrations of one column made at once both land.
//! Every chunk created afterwards then inserts its range row for that column
//! twice and fails on the catalogue's unique key, and with it every history
//! write that needs a new chunk. The registrations are therefore read and
//! written in one transaction under an advisory lock on the hypertable, and a
//! column found registered more than once is unregistered and registered
//! again, which leaves one registration and an unbounded range on every
//! existing chunk until the chunk is converted again.
//!
//! Registered only on an edition with a columnstore, since only a conversion
//! records a range. A column registered beyond the declared ones is left
//! alone: its ranges only ever exclude chunks no row of it lies in.

use crate::{
    bounded_lock_wait::{BoundedWrite, execute_with_bounded_lock_wait},
    error::TemporalStoreError,
    hypertable_policy::HypertableName,
    storage_policy::PolicyReconciliation,
};
use andromeda_postgresql_seaquery::{
    advisory_lock::advisory_xact_lock,
    chunk_skipping::{SkippingColumn, chunk_skipping_columns_statement, disable_chunk_skipping, enable_chunk_skipping},
    statement::Statement,
};
use sea_query::{DynIden, Expr, Iden, Query, SelectStatement, SimpleExpr};
use sqlx::{PgConnection, PgPool, Row};

/// Register every column `hypertable` skips chunks on that is not yet
/// registered, and register once again every column registered more than once.
///
/// # Errors
///
/// Returns [`TemporalStoreError::Database`] when the lock is refused on every
/// attempt, the catalogue cannot be read or a registration fails, including on
/// a database whose sessions do not carry `timescaledb.enable_chunk_skipping`.
pub(crate) async fn reconcile_chunk_skipping(pool: &PgPool, hypertable: HypertableName) -> Result<PolicyReconciliation, TemporalStoreError> {
    Ok(execute_with_bounded_lock_wait(pool, &Registration(hypertable)).await?)
}

/// The registration of the columns a hypertable skips chunks on, read and
/// written in one transaction under the hypertable's registration lock.
struct Registration(HypertableName);

impl BoundedWrite for Registration {
    type Outcome = PolicyReconciliation;

    async fn execute(&self, connection: &mut PgConnection) -> Result<PolicyReconciliation, sqlx::Error> {
        let hypertable: &'static str = self.0.into();
        registration_lock(hypertable).to_query().execute(&mut *connection).await?;
        // Read under the lock: a build that held it may have just registered.
        let registered: Vec<String> = chunk_skipping_columns_statement(hypertable)
            .to_query()
            .fetch_all(&mut *connection)
            .await?
            .iter()
            .map(|row| row.try_get(SkippingColumn.unquoted()))
            .collect::<Result<_, _>>()?;
        let declared = self.0.chunk_skipping_columns();
        let repeated = repeated(&registered);
        let missing = unregistered(&declared, &registered);
        for column in &repeated {
            select_call(&mut *connection, disable_chunk_skipping(hypertable, column)).await?;
            select_call(&mut *connection, enable_chunk_skipping(hypertable, column)).await?;
        }
        for column in &missing {
            select_call(&mut *connection, enable_chunk_skipping(hypertable, &column.to_string())).await?;
        }
        if repeated.is_empty() && missing.is_empty() {
            Ok(PolicyReconciliation::Unchanged)
        } else {
            Ok(PolicyReconciliation::Adjusted)
        }
    }
}

/// Run `SELECT <call>` on `connection`.
async fn select_call(connection: &mut PgConnection, call: SimpleExpr) -> Result<(), sqlx::Error> {
    Query::select().expr(call).to_owned().to_query().execute(connection).await.map(drop)
}

/// The transaction advisory lock that serializes registering the columns
/// `hypertable` skips chunks on.
fn registration_lock(hypertable: &str) -> SelectStatement {
    Query::select()
        .expr(advisory_xact_lock(Expr::val(format!("andromeda:chunk-skipping:{hypertable}"))))
        .to_owned()
}

/// The columns registered more than once, each named once, in the order of
/// their second registration.
fn repeated(registered: &[String]) -> Vec<&str> {
    registered
        .iter()
        .enumerate()
        .filter(|(position, name)| registered.iter().take(*position).filter(|earlier| earlier == name).count() == 1)
        .map(|(_, name)| name.as_str())
        .collect()
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
    use crate::{
        chunk_skipping_enablement::{registration_lock, repeated, unregistered},
        hypertable_policy::HypertableName,
    };
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    /// The catalogue's registration rows naming `names`, in order.
    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn only_the_declared_columns_the_catalogue_lacks_are_registered_in_declaration_order() {
        let declared = HypertableName::AttributeHistory.chunk_skipping_columns();
        let registered = names(&["created_at", "value_number"]);
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

    #[test]
    fn a_column_registered_twice_is_named_once_and_a_column_registered_once_not_at_all() {
        let registered = names(&["observed_at", "created_at", "modified_at", "created_at"]);
        assert_eq!(repeated(&registered), vec!["created_at"]);
    }

    #[test]
    fn a_column_registered_three_times_is_named_once_beside_another_registered_twice() {
        let registered = names(&["created_at", "modified_at", "created_at", "value_number", "created_at", "modified_at"]);
        assert_eq!(repeated(&registered), vec!["created_at", "modified_at"]);
    }

    #[test]
    fn registrations_that_each_name_a_column_once_repeat_nothing() {
        assert_eq!(repeated(&names(&["observed_at", "created_at", "modified_at"])), Vec::<&str>::new());
        assert_eq!(repeated(&[]), Vec::<&str>::new());
    }

    #[test]
    fn the_registration_lock_is_a_transaction_lock_keyed_on_the_hypertable() {
        assert_eq!(
            registration_lock(HypertableName::AttributeHistory.into()).to_string(PostgresQueryBuilder),
            "SELECT pg_advisory_xact_lock(hashtext('andromeda:chunk-skipping:attribute_history'))"
        );
    }
}
