//! The members of an Entity that are not its attributes, and the names a
//! statement reads them under.
//!
//! The identifier, the write count and the clause 4.8 and 4.22 instants live
//! on `entities`; the types, in the order the Entity wrote them, and its
//! Scopes as written (clause 4.18) on `entity_search`. A `current_attributes`
//! row read beside them carries system instants of its own, so the Entity's
//! are read under names of their own.

use andromeda_postgresql_schema::current::{Entities, EntitySearch};
use sea_query::{DynIden, Expr, Iden, IntoIden, SelectStatement, SimpleExpr};

/// One member of an Entity's head, named as a statement reads it.
#[derive(Iden, Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityHead {
    /// `entities.entity_key`, which the Entity's rows are read by.
    #[iden = "entity_key"]
    EntityKey,
    /// `entities.id`.
    #[iden = "id"]
    Id,
    /// `entities.created_at` (clause 4.8).
    #[iden = "entity_created_at"]
    CreatedAt,
    /// `entities.modified_at` (clause 4.8).
    #[iden = "entity_modified_at"]
    ModifiedAt,
    /// `entities.write_count`, which with the key is the version a
    /// read-modify-write states its precondition against.
    #[iden = "entity_write_count"]
    WriteCount,
    /// `entities.expires_at` (clause 4.22).
    #[iden = "entity_expires_at"]
    ExpiresAt,
    /// `entity_search.entity_type_name_keys`, in written order.
    #[iden = "entity_type_name_keys"]
    EntityTypeNameKeys,
    /// `entity_search.verbatim_scope_paths`, the Scopes as written.
    #[iden = "verbatim_scope_paths"]
    VerbatimScopePaths,
}

impl EntityHead {
    /// Every member, in the order a statement projects them.
    pub const ALL: [EntityHead; 8] = [
        EntityHead::EntityKey,
        EntityHead::Id,
        EntityHead::CreatedAt,
        EntityHead::ModifiedAt,
        EntityHead::WriteCount,
        EntityHead::ExpiresAt,
        EntityHead::EntityTypeNameKeys,
        EntityHead::VerbatimScopePaths,
    ];

    /// The column this member is read from, in a statement that names
    /// `entities` and `entity_search` as `entities` and `search`.
    #[must_use]
    pub fn source(self, entities: &DynIden, search: &DynIden) -> SimpleExpr {
        // clone: each member names its relation by the statement's own iden
        let entities = |column: Entities| Expr::col((entities.clone(), column));
        let search = |column: EntitySearch| Expr::col((search.clone(), column));
        match self {
            EntityHead::EntityKey => entities(Entities::EntityKey),
            EntityHead::Id => entities(Entities::Id),
            EntityHead::CreatedAt => entities(Entities::CreatedAt),
            EntityHead::ModifiedAt => entities(Entities::ModifiedAt),
            EntityHead::WriteCount => entities(Entities::WriteCount),
            EntityHead::ExpiresAt => entities(Entities::ExpiresAt),
            EntityHead::EntityTypeNameKeys => search(EntitySearch::EntityTypeNameKeys),
            EntityHead::VerbatimScopePaths => search(EntitySearch::VerbatimScopePaths),
        }
    }
}

/// Add every member of the head to `select` under its own name, read from
/// `entities` and `search` as the statement names them.
pub fn select_head(select: &mut SelectStatement, entities: impl IntoIden, search: impl IntoIden) {
    let (entities, search) = (entities.into_iden(), search.into_iden());
    for member in EntityHead::ALL {
        select.expr_as(member.source(&entities, &search), member);
    }
}

#[cfg(test)]
mod tests {
    use crate::entity_head::select_head;
    use andromeda_postgresql_schema::current::{E, EntitySearch};
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Query};

    /// The Entity's instants are read under names a row's own instants
    /// cannot shadow.
    #[test]
    fn the_head_reads_the_entity_members_under_names_of_their_own() {
        let mut select = Query::select();
        select_head(&mut select, E, EntitySearch::Table);
        assert_eq!(
            select.to_string(PostgresQueryBuilder),
            concat!(
                r#"SELECT "e"."entity_key" AS "entity_key", "e"."id" AS "id", "e"."created_at" AS "entity_created_at", "#,
                r#""e"."modified_at" AS "entity_modified_at", "e"."write_count" AS "entity_write_count", "e"."expires_at" AS "entity_expires_at", "#,
                r#""entity_search"."entity_type_name_keys" AS "entity_type_name_keys", "entity_search"."verbatim_scope_paths" AS "verbatim_scope_paths""#
            )
        );
    }
}
