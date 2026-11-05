//! A stretch of a transaction whose lock waits are bounded, after which the
//! transaction's own bound applies again.
//!
//! `SET LOCAL lock_timeout` lasts to the end of the transaction, so a bound set
//! for one lock would also cut short every later statement of a long
//! transaction. A stretch reads the timeout in force when it begins and puts it
//! back when it ends. A stretch cut short by an error leaves the bound set: the
//! error aborts the transaction, which runs no other statement.

use crate::{
    introspection::current_setting,
    setting::{Setting, SettingScope, set_config},
    statement::Statement,
};
use sea_query::{Iden, Query, SelectStatement};
use sqlx::PgConnection;
use std::time::Duration;

/// The fenced subquery reading the timeout in force, `prior`.
#[derive(Iden, Clone, Copy)]
#[iden = "prior"]
struct Prior;

/// The timeout the fenced subquery read, `timeout`.
#[derive(Iden, Clone, Copy)]
#[iden = "timeout"]
struct Timeout;

/// A stretch of the current transaction whose lock waits end after a bound,
/// holding the timeout it puts back when it ends.
#[derive(Debug)]
#[must_use = "the transaction keeps the bound until the stretch ends"]
pub struct LockWaitBound {
    /// The `lock_timeout` in force when the stretch began, as the server
    /// reports it.
    prior: String,
}

impl LockWaitBound {
    /// End the lock waits of the statements the transaction on `connection`
    /// runs next after `timeout`, until [`LockWaitBound::end`].
    ///
    /// # Errors
    ///
    /// Returns the driver's error when the timeout cannot be read or set.
    pub async fn begin(connection: &mut PgConnection, timeout: Duration) -> Result<LockWaitBound, sqlx::Error> {
        let prior: String = begin_statement(timeout).to_query_scalar().fetch_one(connection).await?;
        Ok(LockWaitBound { prior })
    }

    /// Put back the timeout in force before the stretch.
    ///
    /// # Errors
    ///
    /// Returns the driver's error when the timeout cannot be set.
    pub async fn end(self, connection: &mut PgConnection) -> Result<(), sqlx::Error> {
        restore_statement(&self.prior).to_query().execute(connection).await?;
        Ok(())
    }
}

/// The timeout in force, read before the stretch's own is set:
/// `SELECT "prior"."timeout", set_config('lock_timeout', '<n>ms', true) FROM
/// (SELECT current_setting('lock_timeout') AS "timeout" OFFSET 0) AS "prior"`.
///
/// The fenced subquery is a node of its own, so its row, and the timeout in it,
/// is read before the outer projection sets the new one.
fn begin_statement(timeout: Duration) -> SelectStatement {
    let read = Query::select().expr_as(current_setting(Setting::LockTimeout), Timeout).offset(0).take();
    Query::select()
        .column((Prior, Timeout))
        .expr(set_config(
            Setting::LockTimeout,
            &format!("{}ms", timeout.as_millis()),
            SettingScope::Transaction,
        ))
        .from_subquery(read, Prior)
        .take()
}

/// `SELECT set_config('lock_timeout', <prior>, true)`.
fn restore_statement(prior: &str) -> SelectStatement {
    Query::select().expr(set_config(Setting::LockTimeout, prior, SettingScope::Transaction)).take()
}

#[cfg(test)]
mod tests {
    use crate::{
        lock_wait_bound::{begin_statement, restore_statement},
        statement::rendering::Rendered,
    };
    use pretty_assertions::assert_eq;
    use sea_query::Value;
    use std::time::Duration;

    #[test]
    fn the_stretch_reads_the_timeout_in_force_before_setting_its_own() {
        let (sql, values) = begin_statement(Duration::from_secs(5)).rendered();
        assert_eq!(
            sql,
            r#"SELECT "prior"."timeout", set_config($1, $2, TRUE) FROM (SELECT current_setting($3) AS "timeout" OFFSET 0) AS "prior""#
        );
        assert_eq!(values.0, vec![Value::from("lock_timeout"), Value::from("5000ms"), Value::from("lock_timeout")]);
    }

    #[test]
    fn the_stretch_puts_back_the_timeout_it_read() {
        let (sql, values) = restore_statement("2s").rendered();
        assert_eq!(sql, "SELECT set_config($1, $2, TRUE)");
        assert_eq!(values.0, vec![Value::from("lock_timeout"), Value::from("2s")]);
    }
}
