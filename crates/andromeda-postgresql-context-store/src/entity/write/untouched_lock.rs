//! Locking the identity rows of writes that change no current state.
//!
//! Such a write writes no row, yet it may record history and may state what
//! it assumed of the Entity, so it takes the lock a write of the row would
//! take. Clauses 4.5.7, 4.5.8 and 4.8 read an Entity's history in the order its
//! writes committed, and the identity row lock is what orders them: a write of
//! the current state takes it in its own statement, so every earlier writer of
//! the Entity has committed before the commit asks whether the Entity's history
//! is queued, and before it keys an intent. The lock also reads the row as the
//! writer it waited on left it, so the version it answers is the one the
//! write's precondition is held to. The lock is the one an update of a non-key
//! column takes, so it waits for and blocks every write of the row, and the
//! rows are locked in identifier order, the order no other commit contradicts.

use crate::{
    entity::{locked_entity::LockedEntity, mapper::stored_uri, repo::EntityRepository},
    error::ContextStoreError,
};
use andromeda_context_commit::entity_version::EntityVersion;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_query::entity_sides::holds_current_state;
use andromeda_postgresql_schema::current::Entities;
use andromeda_postgresql_seaquery::{pg_array::is_any_of, statement::Statement};
use andromeda_telemetry::statement::{ObservedStatement, observe_statement};
use sea_query::{Expr, Iden, LockType, Order, Query, SelectStatement};
use sqlx::{Postgres, Row, Transaction};

/// Whether a locked row holds a current state, `current`.
#[derive(Iden, Clone, Copy)]
#[iden = "current"]
struct Current;

/// The lock of the identity row of each Entity `ids` names, answering the
/// version each row stands at and whether it holds a current state:
///
/// ```sql
/// SELECT id, entity_key, write_count, created_at IS NOT NULL AS current
/// FROM entities WHERE id = ANY($ids) ORDER BY id FOR NO KEY UPDATE
/// ```
fn untouched_lock_statement(target: &ContextTarget, ids: &[&Identifier]) -> SelectStatement {
    Query::select()
        .columns([Entities::Id, Entities::EntityKey, Entities::WriteCount])
        .expr_as(holds_current_state(Entities::Table), Current)
        .from(table_of(target, Entities::Table))
        .and_where(is_any_of(Expr::col(Entities::Id), ids.iter().map(|id| id.as_str().to_owned())))
        .order_by(Entities::Id, Order::Asc)
        .lock(LockType::NoKeyUpdate)
        .to_owned()
}

impl EntityRepository {
    /// Lock the identity row of each Entity `ids` names in `tx`, for writes
    /// that change no current state (see [`untouched_lock_statement`]),
    /// answering each locked row. An Entity no row holds has nothing to lock
    /// and is not answered.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError::Database`] when the statement fails, and
    /// [`ContextStoreError::StoredEntity`] when a locked identifier is not a
    /// valid URI.
    pub async fn lock_untouched_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        target: &ContextTarget,
        ids: &[&Identifier],
    ) -> Result<Vec<LockedEntity>, ContextStoreError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let rows = untouched_lock_statement(target, ids).to_query().fetch_all(&mut **tx);
        let rows = observe_statement(ObservedStatement::EntityUntouchedLock, rows).await?;
        rows.iter()
            .map(|row| {
                let version = EntityVersion::new(
                    row.try_get(Iden::to_string(&Entities::EntityKey).as_str())?,
                    row.try_get(Iden::to_string(&Entities::WriteCount).as_str())?,
                );
                let current: bool = row.try_get(Iden::to_string(&Current).as_str())?;
                Ok(LockedEntity {
                    id: stored_uri(row.try_get(Iden::to_string(&Entities::Id).as_str())?)?,
                    current: current.then_some(version),
                })
            })
            .collect()
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{entity::write::untouched_lock::untouched_lock_statement, statement_text::text_of_query};
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_postgresql_seaquery::statement::Statement;
    use pretty_assertions::assert_eq;

    fn vehicle(index: u8) -> Identifier {
        format!("urn:ngsi-ld:Vehicle:{index}").parse().expect("a valid identifier")
    }

    /// The lock an update of `deleted_or_queued_at` takes, in an order every
    /// commit locking several rows agrees on, reading the version a
    /// precondition is held to from the locked row.
    #[test]
    fn the_rows_are_locked_for_a_non_key_update_in_identifier_order_with_their_version() {
        let (first, second) = (vehicle(1), vehicle(2));
        let sql = text_of_query(untouched_lock_statement(&ContextTarget::Live, &[&second, &first]).to_query());
        assert_eq!(
            sql,
            concat!(
                r#"SELECT "id", "entity_key", "write_count", "entities"."created_at" IS NOT NULL AS "current" "#,
                r#"FROM "entities" WHERE "id" = ANY($1) ORDER BY "id" ASC FOR NO KEY UPDATE"#
            )
        );
    }
}
