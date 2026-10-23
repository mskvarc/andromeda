//! Whether an Entity's identity row holds a current state, a Temporal
//! Evolution, or both.
//!
//! One `entities` row serves both APIs. A core deletion (clause 5.6.6) keeps
//! the row while the Temporal Evolution exists, and the Temporal API creates
//! one for an Evolution without a current state, so every statement that
//! reaches an Entity by its identifier or walks the identity rows states the
//! side it serves: a core read the current state, a temporal read the
//! Evolution.

use andromeda_postgresql_schema::current::Entities;
use sea_query::{DynIden, Expr, ExprTrait, IntoIden, SimpleExpr};

/// Whether the identity row `entities` names holds a current state: its
/// clause 4.8 `createdAt` is set.
#[must_use]
pub fn holds_current_state(entities: impl IntoIden) -> SimpleExpr {
    let entities: DynIden = entities.into_iden();
    Expr::col((entities, Entities::CreatedAt)).is_not_null()
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
    use crate::entity_sides::{holds_current_state, holds_evolution};
    use andromeda_postgresql_schema::current::E;
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Query, SimpleExpr};

    fn render(expr: SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    #[test]
    fn a_current_state_is_one_whose_creation_instant_is_set() {
        assert_eq!(render(holds_current_state(E)), r#"SELECT "e"."created_at" IS NOT NULL"#);
    }

    #[test]
    fn a_temporal_evolution_is_one_whose_creation_instant_is_set() {
        assert_eq!(render(holds_evolution(E)), r#"SELECT "e"."evolution_created_at" IS NOT NULL"#);
    }
}
