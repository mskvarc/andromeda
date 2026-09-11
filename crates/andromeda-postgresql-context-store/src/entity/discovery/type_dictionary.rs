//! The `entity_type_names` dictionary as the discovery aggregates read it.
//!
//! An entity carries its types as interned dictionary keys, so every aggregate
//! that reports a type IRI has to expand those keys and join them back to the
//! dictionary row that holds the name. Both halves of that walk live here, and
//! so does the single-type lookup that answers whether a name was ever interned
//! at all.

use crate::{
    entity::{discovery::aggregate_source::S, repo::EntityRepository},
    error::ContextStoreError,
};
use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
use andromeda_postgresql_query::entity_type_id::EntityTypeId;
use andromeda_postgresql_schema::{current::EntitySearch, name_dictionary::EntityTypeNames};
use andromeda_postgresql_seaquery::unnest::unnest_of;
use sea_query::{Expr, ExprTrait, Iden, JoinType, Query, SelectStatement, SimpleExpr};
use std::slice::from_ref;

/// The lateral `unnest(type_ids)` source, aliased `t`; its single column is `t`.
#[derive(Iden, Clone, Copy)]
#[iden = "t"]
pub(super) struct T;

/// The `entity_type_names` dictionary joined to an unnested type key, aliased `n`.
#[derive(Iden, Clone, Copy)]
#[iden = "n"]
pub(super) struct N;

/// The dictionary name column the type aggregates read: `n.name`.
pub(super) fn type_name() -> SimpleExpr {
    Expr::col((N, EntityTypeNames::Name))
}

/// Join the type dictionary to the unnested `type_ids` source, so every
/// type key an entity carries is read back as the IRI it interns.
pub(super) fn join_type_names(statement: &mut SelectStatement) -> &mut SelectStatement {
    statement.join_as(
        JoinType::InnerJoin,
        EntityTypeNames::Table,
        N,
        Expr::col((N, EntityTypeNames::Id)).equals((T, T)),
    )
}

/// Expand the search row's `type_ids` laterally, one row per type key, as
/// `INNER JOIN LATERAL (SELECT UNNEST(s.type_ids) AS t) AS t ON TRUE`.
///
/// Lateral rather than a second `FROM` item: the document row is joined to
/// the search row with an `ON` condition, and a comma-separated `FROM` item
/// cannot be referenced from a later join's `ON`.
pub(super) fn unnest_type_ids(statement: &mut SelectStatement) -> &mut SelectStatement {
    statement.join_lateral(
        JoinType::InnerJoin,
        Query::select()
            .expr_as(SimpleExpr::from(unnest_of(Expr::col((S, EntitySearch::TypeIds)))), T)
            .take(),
        T,
        SimpleExpr::from(Expr::val(true)),
    )
}

impl EntityRepository {
    /// The dictionary key of `entity_type`, when the broker has ever
    /// interned it. A type never interned was never carried by any entity.
    pub(super) async fn type_id(&self, entity_type: &EntityTypeName) -> Result<Option<EntityTypeId>, ContextStoreError> {
        Ok(self.entity_type_names.lookup(from_ref(entity_type)).await?.get(entity_type))
    }
}
