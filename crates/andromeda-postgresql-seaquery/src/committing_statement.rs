//! The last statement of a transaction, sent in one message with the `COMMIT`
//! that ends the transaction.
//!
//! A statement that takes a row lock every concurrent writer queues on holds
//! that lock until its transaction ends. Sent on its own, the lock stays held
//! while the statement's answer travels to the broker and the broker's `COMMIT`
//! travels back, and every writer queued on the row waits out that round trip
//! again. Sent as one simple-query message with the `COMMIT`, the server runs
//! the two back to back, so the lock is held only for the statement and the
//! commit's flush.
//!
//! The message ends with `BEGIN`. The driver's transaction handle counts the
//! transaction it began and cannot be told that the message ended it, so the
//! handle's own commit closes the empty transaction the trailing `BEGIN` opens.
//! Without it, that commit would reach the server outside a transaction, which
//! the server answers with a warning.
//!
//! The server stops a message at its first failing statement. A failing
//! statement stops it before the `COMMIT`, and the transaction rolls back when
//! its handle is dropped. A failing `COMMIT` ends the transaction on the server
//! and stops the message before the `BEGIN`, so the handle's rollback on drop is
//! answered with one warning that no transaction is in progress, and the
//! connection stays usable. A connection lost during the message leaves the
//! outcome of the commit unknown, as one lost during a `COMMIT` of its own does.
//!
//! The simple protocol carries no bound value, so the statement binds none, and
//! one that binds a value is refused when it is built.

use crate::statement::Statement;
use sqlx::{AssertSqlSafe, Decode, Postgres, Row, Transaction, Type};
use thiserror::Error;

/// A statement that ends its transaction, rendered for the simple-query
/// message that carries it with the transaction's `COMMIT`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommittingStatement {
    /// The statement's text, which binds no value.
    text: String,
}

/// A statement refused as the last of its transaction because it binds values
/// the simple protocol cannot carry.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
#[error("a statement sent with the commit of its transaction binds no value, and this one binds {bound}")]
pub struct BoundCommittingStatement {
    /// How many values the statement binds.
    bound: usize,
}

impl CommittingStatement {
    /// `statement` as the last statement of its transaction.
    ///
    /// # Errors
    ///
    /// Returns [`BoundCommittingStatement`] when `statement` binds a value.
    pub fn new(statement: &impl Statement) -> Result<CommittingStatement, BoundCommittingStatement> {
        let (text, values) = statement.rendered();
        match values.0.len() {
            0 => Ok(CommittingStatement { text }),
            bound => Err(BoundCommittingStatement { bound }),
        }
    }

    /// The simple-query message: the statement, the `COMMIT` that ends its
    /// transaction, and the `BEGIN` of the empty transaction the handle closes.
    fn message(&self) -> String {
        format!("{}; COMMIT; BEGIN", self.text)
    }

    /// Run the statement and commit the transaction `transaction` holds open,
    /// returning the first column of the statement's first row.
    ///
    /// The handle is left holding the empty transaction the message opens,
    /// which its own commit closes.
    ///
    /// # Errors
    ///
    /// Returns the driver's error when the statement or the commit fails, and
    /// [`sqlx::Error::RowNotFound`] when the statement returns no row. Every
    /// answer of the message is read before the row is returned, so a failed
    /// commit never hands back the statement's row.
    pub async fn fetch_ending<O>(&self, transaction: &mut Transaction<'_, Postgres>) -> Result<O, sqlx::Error>
    where
        O: for<'row> Decode<'row, Postgres> + Type<Postgres>,
    {
        let row = sqlx::raw_sql(AssertSqlSafe(self.message()))
            .fetch_optional(&mut **transaction)
            .await?
            .ok_or(sqlx::Error::RowNotFound)?;
        row.try_get(0)
    }

    /// Run the statement and commit the transaction `transaction` holds open.
    ///
    /// The handle is left holding the empty transaction the message opens,
    /// which its own commit closes.
    ///
    /// # Errors
    ///
    /// Returns the driver's error when the statement or the commit fails.
    pub async fn execute_ending(&self, transaction: &mut Transaction<'_, Postgres>) -> Result<(), sqlx::Error> {
        sqlx::raw_sql(AssertSqlSafe(self.message())).execute(&mut **transaction).await.map(drop)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        committing_statement::{BoundCommittingStatement, CommittingStatement},
        literal::int_literal,
    };
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Expr, ExprTrait, Query};

    #[test]
    fn the_message_commits_after_the_statement_and_opens_the_transaction_the_handle_closes() {
        let statement = Query::update()
            .table(Alias::new("clock"))
            .value(Alias::new("revision"), Expr::col(Alias::new("revision")).add(int_literal(1)))
            .returning_col(Alias::new("revision"))
            .to_owned();
        let committing = CommittingStatement::new(&statement).expect("a statement binding no value");
        assert_eq!(
            committing.message(),
            r#"UPDATE "clock" SET "revision" = "revision" + (1) RETURNING "revision"; COMMIT; BEGIN"#
        );
    }

    #[test]
    fn a_statement_binding_a_value_is_refused() {
        let statement = Query::update()
            .table(Alias::new("clock"))
            .value(Alias::new("owner"), "urn:a")
            .and_where(Expr::col(Alias::new("revision")).eq(7))
            .to_owned();
        assert_eq!(CommittingStatement::new(&statement), Err(BoundCommittingStatement { bound: 2 }));
    }

    #[test]
    fn a_boolean_and_an_inline_integer_bind_nothing() {
        let statement = Query::update()
            .table(Alias::new("clock"))
            .value(Alias::new("singleton"), true)
            .and_where(Expr::col(Alias::new("key")).eq(int_literal(42)))
            .to_owned();
        let committing = CommittingStatement::new(&statement).expect("a statement binding no value");
        assert_eq!(
            committing.message(),
            r#"UPDATE "clock" SET "singleton" = TRUE WHERE "key" = (42); COMMIT; BEGIN"#
        );
    }
}
