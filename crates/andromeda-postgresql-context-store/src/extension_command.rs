//! Running one extension command (create, update or drop) on a connection of
//! its own, whether the session's role may create an extension, and the
//! refusals a command ends in without failing the reconciliation.
//!
//! `TimescaleDB` refuses its update in a session that has already loaded the
//! old library, so every extension command runs as the first statement of a
//! connection opened for it alone, opened again while it is refused a lock.

use andromeda_postgresql_pool::{
    schema_change_lock::{SCHEMA_CHANGE_LOCK_RETRY, is_lock_not_available},
    sql_state::SqlState,
};
use andromeda_postgresql_seaquery::{
    command::Command,
    introspection::{current_role_is_superuser, extension_trusted},
};
use andromeda_retry::tokio_retry_delay::TokioRetryDelay;
use sea_query::{Expr, ExprTrait, Func, SimpleExpr};
use sqlx::{Connection, PgConnection, postgres::PgConnectOptions};
use strum::IntoEnumIterator;

/// Why the database refused an extension command it did not fail on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::EnumIter)]
pub(crate) enum ExtensionCommandRefusal {
    /// The connecting role does not own the extension, or may not run an
    /// untrusted extension's scripts.
    InsufficientPrivilege,
    /// A `RESTRICT` drop found objects outside the extension depending on its
    /// members.
    DependentObjectsStillExist,
}

impl ExtensionCommandRefusal {
    /// The SQLSTATE the database reports the refusal under.
    const fn sql_state(self) -> SqlState {
        match self {
            ExtensionCommandRefusal::InsufficientPrivilege => SqlState::InsufficientPrivilege,
            ExtensionCommandRefusal::DependentObjectsStillExist => SqlState::DependentObjectsStillExist,
        }
    }
}

/// The refusal a failed extension command reports, `None` for a failure that
/// is none of them.
pub(crate) fn refusal(error: &sqlx::Error) -> Option<ExtensionCommandRefusal> {
    let state = SqlState::of(error)?;
    ExtensionCommandRefusal::iter().find(|refusal| refusal.sql_state() == state)
}

/// Run `command` as the first statement of a connection opened for it, opening
/// another and running it again while it is refused a lock.
pub(crate) async fn run_on_own_connection(options: &PgConnectOptions, command: &impl Command) -> Result<(), sqlx::Error> {
    SCHEMA_CHANGE_LOCK_RETRY
        .run(
            &TokioRetryDelay,
            |error, _| is_lock_not_available(error),
            |_| async move {
                let mut connection = PgConnection::connect_with(options).await?;
                let outcome = command.to_query().execute(&mut connection).await;
                connection.close().await?;
                outcome.map(drop)
            },
        )
        .await
}

/// Whether the session's role may create the extension `name`: as a
/// superuser, or because the shipped version is trusted. Never `NULL`, since an
/// unshipped extension has no trusted flag to read.
pub(crate) fn role_may_create(name: &str) -> SimpleExpr {
    current_role_is_superuser().or(Func::coalesce([extension_trusted(name), Expr::val(false)]))
}

#[cfg(test)]
mod tests {
    use crate::extension_command::{ExtensionCommandRefusal, refusal, role_may_create};
    use andromeda_postgresql_pool::fake_database_error::FakeDatabaseError;
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Query};

    #[test]
    fn a_privilege_refusal_and_a_restricted_drop_are_refusals() {
        assert_eq!(
            refusal(&FakeDatabaseError::new("permission denied", "42501").into_sqlx()),
            Some(ExtensionCommandRefusal::InsufficientPrivilege)
        );
        assert_eq!(
            refusal(&FakeDatabaseError::new("cannot drop extension pg_repack", "2BP01").into_sqlx()),
            Some(ExtensionCommandRefusal::DependentObjectsStillExist)
        );
    }

    #[test]
    fn any_other_failure_is_no_refusal() {
        assert_eq!(refusal(&FakeDatabaseError::new("deadlock", "40P01").into_sqlx()), None);
        assert_eq!(refusal(&FakeDatabaseError::new("syntax", "42601").into_sqlx()), None);
        assert_eq!(refusal(&sqlx::Error::RowNotFound), None);
        assert_eq!(refusal(&sqlx::Error::PoolTimedOut), None);
    }

    #[test]
    fn a_superuser_or_a_trusted_shipped_version_may_create_the_extension() {
        let sql = Query::select().expr(role_may_create("pg_repack")).to_string(PostgresQueryBuilder);
        for clause in [
            r#"(SELECT "rolsuper" FROM "pg_roles" WHERE "rolname" = (current_user))"#,
            r#"COALESCE((SELECT "trusted" FROM "pg_available_extension_versions" WHERE "name" = 'pg_repack'"#,
        ] {
            assert!(sql.contains(clause), "{clause} missing: {sql}");
        }
    }
}
