//! Locking the identity rows a commit writes, before any of its writes run.
//!
//! A commit writes its Entities in several statements, inserts, updates and
//! deletions, each reaching its rows in submission order, while the history
//! drain locks the rows of the Entities it applies in identifier order. Rows
//! two transactions lock in opposite orders deadlock, so a commit locks every
//! existing identity row it writes first, in identifier order: the order the
//! drain and every other commit agree on. Its statements then take no row lock
//! out of that order. Identifiers compare under the `C` collation both the
//! identity rows and the queue declare.
//!
//! A write that changes no current state writes no row, yet it may record
//! history and may state what it assumed of the Entity, so the commit locks its
//! row too. Clauses 4.5.7, 4.5.8 and 4.8 read an Entity's history in the order
//! its writes committed, and the identity row lock is what orders them: every
//! earlier writer of the Entity has committed before the commit asks whether
//! the Entity's history is queued, and before it keys an intent. The lock also
//! reads the row as the writer it waited on left it, so the version it answers
//! is the one the write's precondition is held to.

use crate::{
    entity::{commit_row_lock::CommitRowLock, locked_entity::LockedEntity, mapper::stored_uri, repo::EntityRepository},
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
use sea_query::{Expr, Iden, Order, Query, SelectStatement};
use sqlx::{Postgres, Row, Transaction};

/// Whether a locked row holds a current state, `current`.
#[derive(Iden, Clone, Copy)]
#[iden = "current"]
struct Current;

/// The lock `lock` of the identity row of each Entity `ids` names, in
/// identifier order, answering the version each row stands at and whether it
/// holds a current state:
///
/// ```sql
/// SELECT id, entity_key, write_count, created_at IS NOT NULL AS current
/// FROM entities WHERE id = ANY($ids) ORDER BY id FOR NO KEY UPDATE
/// ```
fn row_lock_statement(target: &ContextTarget, ids: &[&Identifier], lock: CommitRowLock) -> SelectStatement {
    Query::select()
        .columns([Entities::Id, Entities::EntityKey, Entities::WriteCount])
        .expr_as(holds_current_state(Entities::Table), Current)
        .from(table_of(target, Entities::Table))
        .and_where(is_any_of(Expr::col(Entities::Id), ids.iter().map(|id| id.as_str().to_owned())))
        .order_by(Entities::Id, Order::Asc)
        .lock(lock.lock_type())
        .to_owned()
}

impl EntityRepository {
    /// Lock with `lock` the identity row of each Entity `ids` names in `tx`,
    /// in identifier order, before the commit's writes run (see
    /// [`row_lock_statement`]), answering each locked row. An Entity no row
    /// holds has nothing to lock and is not answered.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError::Database`] when the statement fails, and
    /// [`ContextStoreError::StoredEntity`] when a locked identifier is not a
    /// valid URI.
    pub async fn lock_commit_rows_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        target: &ContextTarget,
        ids: &[&Identifier],
        lock: CommitRowLock,
    ) -> Result<Vec<LockedEntity>, ContextStoreError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let rows = row_lock_statement(target, ids, lock).to_query().fetch_all(&mut **tx);
        let rows = observe_statement(ObservedStatement::EntityCommitLock, rows).await?;
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
    use crate::{
        entity::{commit_row_lock::CommitRowLock, write::row_lock::row_lock_statement},
        statement_text::text_of_query,
    };
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_postgresql_seaquery::statement::Statement;
    use pretty_assertions::assert_eq;

    fn vehicle(index: u8) -> Identifier {
        format!("urn:ngsi-ld:Vehicle:{index}").parse().expect("a valid identifier")
    }

    fn rendered(lock: CommitRowLock) -> String {
        let (first, second) = (vehicle(1), vehicle(2));
        text_of_query(row_lock_statement(&ContextTarget::Live, &[&second, &first], lock).to_query())
    }

    /// A commit that writes rows takes the lock an update of a non-key column
    /// takes, in the identifier order every commit and the drain agree on,
    /// reading the version a precondition is held to.
    #[test]
    fn a_writing_commit_locks_its_rows_for_a_non_key_update_in_identifier_order() {
        assert_eq!(
            rendered(CommitRowLock::Writing),
            concat!(
                r#"SELECT "id", "entity_key", "write_count", "entities"."created_at" IS NOT NULL AS "current" "#,
                r#"FROM "entities" WHERE "id" = ANY($1) ORDER BY "id" ASC FOR NO KEY UPDATE"#
            )
        );
    }

    /// A commit that deletes a row holds the lock its deletion takes from the
    /// start, so no lock is raised once others wait on the row.
    #[test]
    fn a_deleting_commit_locks_its_rows_for_update() {
        assert!(
            rendered(CommitRowLock::Deleting).ends_with(r#"ORDER BY "id" ASC FOR UPDATE"#),
            "{}",
            rendered(CommitRowLock::Deleting)
        );
    }
}
