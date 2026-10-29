//! Taking back the Temporal Evolution a create established when its commit
//! does not record that evolution's history in the same transaction.
//!
//! A create establishes the evolution only for an Entity whose history the
//! commit records inline, and decides that before it knows whether the
//! history lands inline: an Entity whose earlier history still waits in the
//! intent queue has its history queued behind it, and a write whose history
//! interprets to no typed intent records none. Such an Entity's identity row
//! and memberships go back to what a create that established nothing writes,
//! so the evolution comes into being where the history that creates it is
//! applied.

use crate::{entity::repo::EntityRepository, error::ContextStoreError};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::current::{Entities, EntityTypeMembers};
use andromeda_postgresql_seaquery::{
    literal::false_literal,
    pg_array::{any, text_array},
    statement::Statement,
};
use andromeda_telemetry::statement::{StatementName, observe_statement};
use sea_query::{CommonTableExpression, Expr, ExprTrait, Iden, Keyword, Query, WithClause, WithQuery};
use sqlx::{Postgres, Transaction};

/// The statement name the withdrawal is observed under.
const EVOLUTION_WITHDRAWAL: StatementName = StatementName("entity-evolution-withdrawal");

/// The identity rows the withdrawal cleared, `withdrawn`.
#[derive(Iden, Clone, Copy)]
#[iden = "withdrawn"]
struct Withdrawn;

/// The withdrawal of the Temporal Evolution the create established for each
/// Entity `ids` names: the identity row's evolution columns cleared and its
/// memberships taken off the Evolution's side.
///
/// ```sql
/// WITH withdrawn AS (UPDATE entities SET evolution_created_at = NULL, evolution_modified_at = NULL,
///                    evolution_type_name_keys = NULL WHERE id = ANY($1) RETURNING entity_key)
/// UPDATE entity_type_members SET is_evolution_member = FALSE WHERE entity_key IN (SELECT entity_key FROM withdrawn)
/// ```
///
/// The create minted each row with the current state, so every membership
/// keeps its current flag and none is left belonging to neither side.
fn withdrawal_statement(target: &ContextTarget, ids: &[Identifier]) -> WithQuery {
    let ids = ids.iter().map(Identifier::as_str).collect::<Vec<_>>();
    let mut withdrawn = Query::update();
    withdrawn
        .table(table_of(target, Entities::Table))
        .values([
            (Entities::EvolutionCreatedAt, Keyword::Null.into()),
            (Entities::EvolutionModifiedAt, Keyword::Null.into()),
            (Entities::EvolutionTypeNameKeys, Keyword::Null.into()),
        ])
        .and_where(Expr::col(Entities::Id).eq(any(text_array(&ids))))
        .returning(Query::returning().column(Entities::EntityKey));
    let mut memberships = Query::update();
    memberships
        .table(table_of(target, EntityTypeMembers::Table))
        .value(EntityTypeMembers::IsEvolutionMember, false_literal())
        .and_where(Expr::col(EntityTypeMembers::EntityKey).in_subquery(Query::select().column(Entities::EntityKey).from(Withdrawn).take()));
    let mut with = WithClause::new();
    with.cte(CommonTableExpression::new().table_name(Withdrawn).query(withdrawn).to_owned());
    with.query(memberships)
}

impl EntityRepository {
    /// Take back the Temporal Evolution the create earlier in `tx` established
    /// for each Entity `ids` names (see [`withdrawal_statement`]).
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError::Database`] when the statement fails.
    pub async fn withdraw_established_evolutions_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        target: &ContextTarget,
        ids: &[Identifier],
    ) -> Result<(), ContextStoreError> {
        if ids.is_empty() {
            return Ok(());
        }
        let done = withdrawal_statement(target, ids).to_query().execute(&mut **tx);
        observe_statement(EVOLUTION_WITHDRAWAL, done).await?;
        Ok(())
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{entity::write::evolution_withdrawal::withdrawal_statement, statement_text::text_of_query};
    use andromeda_context_space::{context_target::ContextTarget, snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_postgresql_seaquery::statement::Statement;

    fn vehicle() -> Identifier {
        "urn:ngsi-ld:Vehicle:1".parse().expect("a valid identifier")
    }

    #[test]
    fn a_withdrawal_clears_the_evolution_columns_and_the_evolution_flag_of_the_same_rows() {
        let sql = text_of_query(withdrawal_statement(&ContextTarget::Live, &[vehicle()]).to_query());
        pretty_assertions::assert_eq!(
            sql,
            concat!(
                r#"WITH "withdrawn" AS (UPDATE "entities" SET "evolution_created_at" = NULL, "evolution_modified_at" = NULL, "evolution_type_name_keys" = NULL "#,
                r#"WHERE "id" = ANY($1) RETURNING "entity_key") "#,
                r#"UPDATE "entity_type_members" SET "is_evolution_member" = FALSE WHERE "entity_key" IN (SELECT "entity_key" FROM "withdrawn")"#
            )
        );
    }

    #[test]
    fn a_snapshot_branch_withdraws_in_its_own_schema() {
        let key = SnapshotKey::try_from(5).expect("positive key");
        let id = "urn:ngsi-ld:Snapshot:s".parse().expect("valid snapshot uri");
        let target = ContextTarget::Snapshot(SnapshotTarget::staging(key, id));
        let sql = text_of_query(withdrawal_statement(&target, &[vehicle()]).to_query());
        assert!(sql.contains(r#"UPDATE "snap_5_stg"."entities""#), "{sql}");
        assert!(sql.contains(r#"UPDATE "snap_5_stg"."entity_type_members""#), "{sql}");
    }
}
