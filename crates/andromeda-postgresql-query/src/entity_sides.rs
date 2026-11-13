//! Whether an Entity's identity row holds a current state, a Temporal
//! Evolution, or both.
//!
//! One `entities` row serves both APIs. A core deletion (clause 5.6.6) keeps
//! the row while the Temporal Evolution exists, and the Temporal API creates
//! one for an Evolution without a current state, so every statement that
//! reaches an Entity by its identifier or walks the identity rows states the
//! side it serves: a core read the current state, a temporal read the
//! Evolution.
//!
//! A core read serves a current state only while it stands. Clause 4.22 makes
//! an Entity invalid from its `expiresAt` on, before the expiry sweep deletes
//! it, while a write still reaches it until then.

use crate::expiry::unexpired;
use andromeda_postgresql_schema::current::Entities;
use andromeda_postgresql_seaquery::{branch::qualified, clock::now};
use sea_query::{DynIden, Expr, ExprTrait, IntoIden, Query, SimpleExpr};

/// Whether the identity row `entities` names holds a current state: its
/// clause 4.8 `createdAt` is set.
#[must_use]
pub fn holds_current_state(entities: impl IntoIden) -> SimpleExpr {
    let entities: DynIden = entities.into_iden();
    Expr::col((entities, Entities::CreatedAt)).is_not_null()
}

/// Whether the identity row `entities` names holds a current state that
/// stands: it holds one, and the Entity's clause 4.22 `expiresAt`, when it has
/// one, is still ahead of the transaction's instant.
#[must_use]
pub fn holds_standing_state(entities: impl IntoIden) -> SimpleExpr {
    let entities: DynIden = entities.into_iden();
    holds_current_state(entities.clone()).and(unexpired(Expr::col((entities, Entities::ExpiresAt))))
}

/// Whether the Entity keyed `key` in the branch `schema` has not expired: it is
/// none of the Entities whose clause 4.22 `expiresAt` has passed.
///
/// The expired Entities are the few the expiry sweep has yet to delete, read
/// through the partial index on `expires_at`, so a statement that never joins
/// `entities` tests each row it walks against that one small set instead of
/// joining every row.
#[must_use]
pub fn not_expired(key: SimpleExpr, schema: Option<&DynIden>) -> SimpleExpr {
    let expired = Query::select()
        .column(Entities::EntityKey)
        .from(qualified(schema, Entities::Table))
        .and_where(Expr::col(Entities::ExpiresAt).lte(now()))
        .take();
    key.not_in_subquery(expired)
}

/// Whether the identity row `entities` names holds a Temporal Evolution: its
/// clause 4.8 `createdAt` is set.
#[must_use]
pub fn holds_evolution(entities: impl IntoIden) -> SimpleExpr {
    let entities: DynIden = entities.into_iden();
    Expr::col((entities, Entities::EvolutionCreatedAt)).is_not_null()
}

#[cfg(test)]
mod tests {
    use crate::entity_sides::{holds_current_state, holds_evolution, holds_standing_state, not_expired};
    use andromeda_postgresql_schema::current::E;
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, DynIden, Expr, IntoIden, PostgresQueryBuilder, Query, SimpleExpr};

    fn render(expr: SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    #[test]
    fn a_current_state_is_one_whose_creation_instant_is_set() {
        assert_eq!(render(holds_current_state(E)), r#"SELECT "e"."created_at" IS NOT NULL"#);
    }

    /// Clause 4.22: an Entity past its `expiresAt` is invalid before the sweep
    /// deletes it.
    #[test]
    fn a_standing_state_is_a_current_one_whose_expiry_is_ahead_or_absent() {
        assert_eq!(
            render(holds_standing_state(E)),
            r#"SELECT "e"."created_at" IS NOT NULL AND ("e"."expires_at" IS NULL OR "e"."expires_at" > NOW())"#
        );
    }

    #[test]
    fn an_unjoined_entity_is_tested_against_the_expired_set_of_its_branch() {
        let schema: DynIden = Alias::new("snapshot_1").into_iden();
        assert_eq!(
            render(not_expired(Expr::col(("selected", "entity_key")), Some(&schema))),
            concat!(
                r#"SELECT "selected"."entity_key" NOT IN "#,
                r#"(SELECT "entity_key" FROM "snapshot_1"."entities" WHERE "expires_at" <= NOW())"#
            )
        );
    }

    #[test]
    fn a_temporal_evolution_is_one_whose_creation_instant_is_set() {
        assert_eq!(render(holds_evolution(E)), r#"SELECT "e"."evolution_created_at" IS NOT NULL"#);
    }
}
