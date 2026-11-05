//! Locking the identity rows of writes that record history and change no
//! current state.
//!
//! Clauses 4.5.7, 4.5.8 and 4.8 read an Entity's history in the order its
//! writes committed, and the identity row lock is what orders them: a write of
//! the current state takes it in its own statement, so every earlier writer of
//! the Entity has committed before the commit asks whether the Entity's history
//! is queued, and before it keys an intent. A write that only records history
//! writes no row, so it takes the lock here first. The lock is the one an
//! update of a non-key column takes, so it waits for and blocks every write of
//! the row, and the rows are locked in identifier order, the order no other
//! commit contradicts.

use crate::{entity::repo::EntityRepository, error::ContextStoreError};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::current::Entities;
use andromeda_postgresql_seaquery::{pg_array::is_any_of, statement::Statement};
use andromeda_telemetry::statement::{ObservedStatement, observe_statement};
use sea_query::{Expr, LockType, Order, Query, SelectStatement};
use sqlx::{Postgres, Transaction};

/// The lock of the identity row of each Entity `ids` names:
///
/// ```sql
/// SELECT entity_key FROM entities WHERE id = ANY($ids) ORDER BY id FOR NO KEY UPDATE
/// ```
fn history_lock_statement(target: &ContextTarget, ids: &[&Identifier]) -> SelectStatement {
    Query::select()
        .column(Entities::EntityKey)
        .from(table_of(target, Entities::Table))
        .and_where(is_any_of(Expr::col(Entities::Id), ids.iter().map(|id| id.as_str().to_owned())))
        .order_by(Entities::Id, Order::Asc)
        .lock(LockType::NoKeyUpdate)
        .to_owned()
}

impl EntityRepository {
    /// Lock the identity row of each Entity `ids` names in `tx`, for writes
    /// that record its history and write no row of it (see
    /// [`history_lock_statement`]). An Entity no row holds has nothing to
    /// lock.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError::Database`] when the statement fails.
    pub async fn lock_for_history_in_tx(tx: &mut Transaction<'_, Postgres>, target: &ContextTarget, ids: &[&Identifier]) -> Result<(), ContextStoreError> {
        if ids.is_empty() {
            return Ok(());
        }
        let done = history_lock_statement(target, ids).to_query().execute(&mut **tx);
        observe_statement(ObservedStatement::EntityHistoryLock, done).await?;
        Ok(())
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{entity::write::history_lock::history_lock_statement, statement_text::text_of_query};
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_postgresql_seaquery::statement::Statement;
    use pretty_assertions::assert_eq;

    fn vehicle(index: u8) -> Identifier {
        format!("urn:ngsi-ld:Vehicle:{index}").parse().expect("a valid identifier")
    }

    /// The lock an update of `deleted_or_queued_at` takes, in an order every
    /// commit locking several rows agrees on.
    #[test]
    fn the_rows_are_locked_for_a_non_key_update_in_identifier_order() {
        let (first, second) = (vehicle(1), vehicle(2));
        let sql = text_of_query(history_lock_statement(&ContextTarget::Live, &[&second, &first]).to_query());
        assert_eq!(
            sql,
            r#"SELECT "entity_key" FROM "entities" WHERE "id" = ANY($1) ORDER BY "id" ASC FOR NO KEY UPDATE"#
        );
    }
}
