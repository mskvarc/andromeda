//! The current side of the `entity_type_members` rows each set-write keeps in
//! step with the types the search row carries.
//!
//! A type selection pages by walking the membership key in identifier order
//! (clauses 4.17 and 5.5.9), so every write that sets an Entity's types states
//! them there too, in the same statement as the search row: a create lands one
//! row per type, and an update moves only the types that changed, which a
//! value-only write never does. A row the Temporal Evolution holds stays when
//! the current state drops its type, with the current flag cleared.
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
        staging::{from_inserted, projected},
        statement_ctes::{Established, Ins, Srch},
    },
    error::ContextStoreError,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_branch::schema_iden;
use andromeda_postgresql_query::{
    type_membership::Memberships,
    type_membership_moves::{Member, MembershipRemoval, carried_among, membership_insert, membership_removal},
};
use andromeda_postgresql_schema::{
    current::{Entities, EntitySearch},
    type_membership::MembershipSide,
};
use andromeda_postgresql_seaquery::literal::false_literal;
use sea_query::{DynIden, Expr, ExprTrait, Iden, InsertStatement, Query};

/// The membership CTE of a create or an update, `mbrs`, whose only purpose is
/// to run.
#[derive(Iden, Clone, Copy)]
#[iden = "mbrs"]
pub(super) struct Mbrs;

/// The CTE of an update that deletes the memberships of the types an Entity
/// no longer carries and its Temporal Evolution does not hold, `strays`,
/// whose only purpose is to run.
#[derive(Iden, Clone, Copy)]
#[iden = "strays"]
pub(super) struct Strays;

/// The CTE of an update that clears the current flag of the types an Entity
/// no longer carries and its Temporal Evolution holds, `unflagged`, whose only
/// purpose is to run.
#[derive(Iden, Clone, Copy)]
#[iden = "unflagged"]
pub(super) struct Unflagged;

/// The current side of the membership relation in the branch `schema` names.
const fn current(schema: Option<&DynIden>) -> Memberships<'_> {
    Memberships {
        side: MembershipSide::Current,
        schema,
    }
}

/// The membership rows of the Entities a create just minted, read from the
/// source the other child inserts read (`FROM ins JOIN proj ...`), each on the
/// Evolution's side too where the create established the Temporal Evolution
/// under the same types.
///
/// # Errors
///
/// Returns [`ContextStoreError::StatementBuild`] if the projection ever stops
/// matching the column list, which the fixed shape makes unreachable.
pub(crate) fn created_memberships(target: &ContextTarget) -> Result<InsertStatement, ContextStoreError> {
    let schema = schema_iden(target);
    let member = Member {
        key: Expr::col((Ins, Entities::EntityKey)),
        id: Expr::col((Ins, Entities::Id)),
    };
    Ok(membership_insert(
        current(schema.as_ref()),
        from_inserted(),
        projected(EntitySearch::EntityTypeNameKeys),
        member,
        None,
        Expr::col((Ins, Established)),
    )?)
}

/// The membership rows an update adds: the types the rewritten search row
/// holds that the row it replaced did not.
///
/// # Errors
///
/// As [`created_memberships`].
pub(super) fn added_memberships(target: &ContextTarget) -> Result<InsertStatement, ContextStoreError> {
    let schema = schema_iden(target);
    let memberships = current(schema.as_ref());
    let member = Member {
        key: Expr::col((Srch, EntitySearch::EntityKey)),
        id: Expr::col((Srch, Entities::Id)),
    };
    let added = carried_among(Expr::col((Srch, SearchImage::OldTypes))).not();
    Ok(membership_insert(
        memberships,
        Query::select().from(Srch).take(),
        Expr::col((Srch, SearchImage::NewTypes)),
        member,
        Some(added),
        false_literal(),
    )?)
}

/// The membership rows an update takes off the current side: the types the
/// replaced search row held that the rewritten one does not, found by the
/// membership key.
pub(super) fn dropped_memberships(target: &ContextTarget) -> MembershipRemoval {
    let schema = schema_iden(target);
    membership_removal(
        current(schema.as_ref()),
        Query::select().from(Srch).take(),
        Expr::col((Srch, SearchImage::OldTypes)),
        Expr::col((Srch, SearchImage::NewTypes)),
        Expr::col((Srch, Entities::Id)),
    )
}

#[cfg(test)]
mod tests {
    use crate::entity::write::type_membership::{added_memberships, created_memberships, dropped_memberships};
    use andromeda_context_space::context_target::ContextTarget;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    /// A re-created Entity whose Temporal Evolution kept its types finds
    /// their rows and sets the current flag on them.
    #[test]
    fn a_create_lands_one_membership_row_per_projected_type() {
        let sql = created_memberships(&ContextTarget::Live)
            .expect("the membership insert builds")
            .to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            concat!(
                r#"INSERT INTO "entity_type_members" ("entity_key", "entity_type_name_key", "id", "is_current_member", "is_evolution_member") "#,
                r#"SELECT "ins"."entity_key", "carried"."entity_type_name_key", "ins"."id", TRUE, "ins"."established" FROM "ins" "#,
                r#"JOIN "proj" ON "proj"."id" = "ins"."id" "#,
                r#"INNER JOIN LATERAL (SELECT UNNEST("proj"."entity_type_name_keys") AS "entity_type_name_key") AS "carried" ON TRUE "#,
                r#"ON CONFLICT ("entity_type_name_key", "id") DO UPDATE SET "is_current_member" = TRUE"#
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
            sql.contains(r#"SELECT "srch"."entity_key", "carried"."entity_type_name_key", "srch"."id", TRUE, FALSE FROM "srch""#),
            "{sql}"
        );
        assert!(sql.contains(r#"UNNEST("srch"."new_types")"#), "{sql}");
        assert!(sql.contains(r#"NOT "carried"."entity_type_name_key" = ANY("srch"."old_types")"#), "{sql}");
    }

    /// An update takes off the current side, by the membership key, only the
    /// types the replaced search row held and the rewritten one no longer
    /// does: a row the Temporal Evolution holds keeps it with the flag
    /// cleared, any other goes.
    #[test]
    fn an_update_drops_only_the_types_the_rewritten_row_no_longer_holds() {
        let removal = dropped_memberships(&ContextTarget::Live);
        let cleared = removal.cleared.to_string(PostgresQueryBuilder);
        let deleted = removal.deleted.to_string(PostgresQueryBuilder);
        assert!(
            cleared.starts_with(
                r#"UPDATE "entity_type_members" SET "is_current_member" = FALSE WHERE ("entity_type_name_key", "id") IN (SELECT "carried"."entity_type_name_key", "srch"."id" FROM "srch""#
            ),
            "{cleared}"
        );
        assert!(cleared.ends_with(r#" AND "is_evolution_member""#), "{cleared}");
        assert!(
            deleted.starts_with(
                r#"DELETE FROM "entity_type_members" WHERE ("entity_type_name_key", "id") IN (SELECT "carried"."entity_type_name_key", "srch"."id" FROM "srch""#
            ),
            "{deleted}"
        );
        assert!(deleted.ends_with(r#" AND (NOT "is_evolution_member")"#), "{deleted}");
        for sql in [cleared, deleted] {
            assert!(sql.contains(r#"UNNEST("srch"."old_types")"#), "{sql}");
            assert!(sql.contains(r#"NOT "carried"."entity_type_name_key" = ANY("srch"."new_types")"#), "{sql}");
        }
    }

    /// The moves never read `entity_search`: the statement's snapshot of it can
    /// predate a concurrent writer the search write waited for.
    #[test]
    fn the_membership_moves_never_read_the_search_relation() {
        let added = added_memberships(&ContextTarget::Live).expect("builds").to_string(PostgresQueryBuilder);
        let dropped = dropped_memberships(&ContextTarget::Live);
        for sql in [
            added,
            dropped.cleared.to_string(PostgresQueryBuilder),
            dropped.deleted.to_string(PostgresQueryBuilder),
        ] {
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
        let dropped = dropped_memberships(&target);
        assert!(added.contains(r#"INSERT INTO "snap_5_stg"."entity_type_members""#), "{added}");
        assert!(
            dropped
                .cleared
                .to_string(PostgresQueryBuilder)
                .contains(r#"UPDATE "snap_5_stg"."entity_type_members""#),
            "{dropped:?}"
        );
        assert!(
            dropped
                .deleted
                .to_string(PostgresQueryBuilder)
                .contains(r#"DELETE FROM "snap_5_stg"."entity_type_members""#),
            "{dropped:?}"
        );
    }
}
