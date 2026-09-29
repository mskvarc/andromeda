//! The `entity_type_members` rows each set-write keeps in step with the types
//! the search row carries.
//!
//! A type selection pages by walking the membership key in identifier order
//! (clauses 4.17 and 5.5.9), so every write that sets an Entity's types states
//! them there too, in the same statement as the search row: a create lands one
//! row per type, and an update moves only the types that changed, which a
//! value-only write never does. A delete needs nothing here; the rows cascade
//! from `entities`.
//!
//! An update reads the types an Entity had and has from the images its search
//! row write returned ([`SearchImage`]), never from `entity_search` itself: the
//! row the write replaced is the one it found after waiting on any concurrent
//! writer, while the relation read in the same `WITH` is the statement's
//! snapshot, which can predate that writer's commit. Moving memberships from the
//! snapshot would keep a type the concurrent writer had already dropped, or
//! insert a membership the concurrent writer had already added.

use crate::{
    entity::write::{
        search_image::SearchImage,
        staging::{Ins, Srch, from_inserted, projected},
    },
    error::ContextStoreError,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::current::{Entities, EntitySearch, EntityTypeMembers};
use andromeda_postgresql_seaquery::{pg_array::any, unnest::unnest_of};
use sea_query::{DeleteStatement, Expr, ExprTrait, Iden, InsertStatement, JoinType, Query, SelectStatement, SimpleExpr};

/// The membership CTE of a create or an update, `mbrs`, whose only purpose is
/// to run.
#[derive(Iden, Clone, Copy)]
#[iden = "mbrs"]
pub(super) struct Mbrs;

/// The CTE of an update that drops the types an Entity no longer carries,
/// `strays`, whose only purpose is to run.
#[derive(Iden, Clone, Copy)]
#[iden = "strays"]
pub(super) struct Strays;

/// The lateral that unnests one type-key array, one row per key.
#[derive(Iden, Clone, Copy)]
#[iden = "carried"]
struct Carried;

/// The membership rows of the Entities `rows` names: one per key of `keys`
/// (for which `admitted` holds, when given), keyed by `entity_key` and
/// identified by `id`.
///
/// # Errors
///
/// Returns [`ContextStoreError::Internal`] if the projection ever stops
/// matching the column list, which the fixed shape here makes unreachable.
fn membership_insert(
    target: &ContextTarget,
    mut rows: SelectStatement,
    keys: SimpleExpr,
    entity_key: SimpleExpr,
    id: SimpleExpr,
    admitted: Option<SimpleExpr>,
) -> Result<InsertStatement, ContextStoreError> {
    unnest_keys(&mut rows, keys);
    rows.expr(entity_key).expr(carried_key()).expr(id);
    if let Some(admitted) = admitted {
        rows.and_where(admitted);
    }
    let mut insert = Query::insert();
    insert
        .into_table(table_of(target, EntityTypeMembers::Table))
        .columns([EntityTypeMembers::EntityKey, EntityTypeMembers::EntityTypeNameKey, EntityTypeMembers::Id])
        .select_from(rows)
        .map_err(|error| ContextStoreError::Internal(error.to_string()))?;
    Ok(insert)
}

/// The membership rows of the Entities a create just minted, read from the
/// source the other child inserts read (`FROM ins JOIN proj ...`).
///
/// # Errors
///
/// As [`membership_insert`].
pub(crate) fn created_memberships(target: &ContextTarget) -> Result<InsertStatement, ContextStoreError> {
    membership_insert(
        target,
        from_inserted(),
        projected(EntitySearch::EntityTypeNameKeys),
        Expr::col((Ins, Entities::EntityKey)),
        Expr::col((Ins, Entities::Id)),
        None,
    )
}

/// The membership rows an update adds: the types the rewritten search row
/// holds that the row it replaced did not.
///
/// # Errors
///
/// As [`membership_insert`].
pub(super) fn added_memberships(target: &ContextTarget) -> Result<InsertStatement, ContextStoreError> {
    membership_insert(
        target,
        Query::select().from(Srch).take(),
        Expr::col((Srch, SearchImage::NewTypes)),
        Expr::col((Srch, EntitySearch::EntityKey)),
        Expr::col((Srch, Entities::Id)),
        Some(carried_key().eq(any(Expr::col((Srch, SearchImage::OldTypes)))).not()),
    )
}

/// The membership rows an update removes: the types the replaced search row
/// held that the rewritten one does not, found by the membership key.
pub(super) fn dropped_memberships(target: &ContextTarget) -> DeleteStatement {
    let mut stale = Query::select().from(Srch).take();
    unnest_keys(&mut stale, Expr::col((Srch, SearchImage::OldTypes)));
    stale
        .expr(carried_key())
        .expr(Expr::col((Srch, Entities::Id)))
        .and_where(carried_key().eq(any(Expr::col((Srch, SearchImage::NewTypes)))).not());
    Query::delete()
        .from_table(table_of(target, EntityTypeMembers::Table))
        .and_where(Expr::tuple([Expr::col(EntityTypeMembers::EntityTypeNameKey), Expr::col(EntityTypeMembers::Id)]).in_subquery(stale.take()))
        .take()
}

/// Join one row per key of `keys` to `rows`, as
/// `INNER JOIN LATERAL (SELECT UNNEST(keys) AS entity_type_name_key) AS carried ON TRUE`.
fn unnest_keys(rows: &mut SelectStatement, keys: SimpleExpr) {
    rows.join_lateral(
        JoinType::InnerJoin,
        Query::select()
            .expr_as(SimpleExpr::from(unnest_of(keys)), EntityTypeMembers::EntityTypeNameKey)
            .take(),
        Carried,
        SimpleExpr::from(Expr::val(true)),
    );
}

/// The key the lateral unnested: `carried.entity_type_name_key`.
fn carried_key() -> SimpleExpr {
    Expr::col((Carried, EntityTypeMembers::EntityTypeNameKey))
}

#[cfg(test)]
mod tests {
    use crate::entity::write::type_membership::{added_memberships, created_memberships, dropped_memberships};
    use andromeda_context_space::context_target::ContextTarget;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn a_create_lands_one_membership_row_per_projected_type() {
        let sql = created_memberships(&ContextTarget::Live)
            .expect("the membership insert builds")
            .to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            concat!(
                r#"INSERT INTO "entity_type_members" ("entity_key", "entity_type_name_key", "id") "#,
                r#"SELECT "ins"."entity_key", "carried"."entity_type_name_key", "ins"."id" FROM "ins" "#,
                r#"JOIN "proj" ON "proj"."id" = "ins"."id" "#,
                r#"INNER JOIN LATERAL (SELECT UNNEST("proj"."entity_type_name_keys") AS "entity_type_name_key") AS "carried" ON TRUE"#
            )
        );
    }

    /// An update adds only the types the rewritten search row holds and the
    /// replaced one lacked, read from the images the search write returned.
    #[test]
    fn an_update_adds_only_the_types_the_replaced_row_lacked() {
        let sql = added_memberships(&ContextTarget::Live)
            .expect("the membership insert builds")
            .to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(r#"SELECT "srch"."entity_key", "carried"."entity_type_name_key", "srch"."id" FROM "srch""#),
            "{sql}"
        );
        assert!(sql.contains(r#"UNNEST("srch"."new_types")"#), "{sql}");
        assert!(sql.contains(r#"NOT "carried"."entity_type_name_key" = ANY("srch"."old_types")"#), "{sql}");
    }

    /// An update drops, by the membership key, only the types the replaced
    /// search row held and the rewritten one no longer does.
    #[test]
    fn an_update_drops_only_the_types_the_rewritten_row_no_longer_holds() {
        let sql = dropped_memberships(&ContextTarget::Live).to_string(PostgresQueryBuilder);
        assert!(
            sql.starts_with(
                r#"DELETE FROM "entity_type_members" WHERE ("entity_type_name_key", "id") IN (SELECT "carried"."entity_type_name_key", "srch"."id" FROM "srch""#
            ),
            "{sql}"
        );
        assert!(sql.contains(r#"UNNEST("srch"."old_types")"#), "{sql}");
        assert!(sql.contains(r#"NOT "carried"."entity_type_name_key" = ANY("srch"."new_types")"#), "{sql}");
    }

    /// The moves never read `entity_search`: the statement's snapshot of it can
    /// predate a concurrent writer the search write waited for.
    #[test]
    fn the_membership_moves_never_read_the_search_relation() {
        let added = added_memberships(&ContextTarget::Live).expect("builds").to_string(PostgresQueryBuilder);
        let dropped = dropped_memberships(&ContextTarget::Live).to_string(PostgresQueryBuilder);
        for sql in [added, dropped] {
            assert!(!sql.contains("entity_search"), "{sql}");
        }
    }

    /// A Snapshot branch keeps its memberships in its own schema.
    #[test]
    fn a_snapshot_branch_keeps_its_memberships_in_the_branch_schema() {
        use andromeda_context_space::{snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
        let key = SnapshotKey::try_from(5).expect("positive key");
        let target = ContextTarget::Snapshot(SnapshotTarget::staging(key, "urn:ngsi-ld:Snapshot:s".parse().expect("valid snapshot uri")));
        let added = added_memberships(&target).expect("builds").to_string(PostgresQueryBuilder);
        let dropped = dropped_memberships(&target).to_string(PostgresQueryBuilder);
        assert!(added.contains(r#"INSERT INTO "snap_5_stg"."entity_type_members""#), "{added}");
        assert!(dropped.contains(r#"DELETE FROM "snap_5_stg"."entity_type_members""#), "{dropped}");
    }
}
