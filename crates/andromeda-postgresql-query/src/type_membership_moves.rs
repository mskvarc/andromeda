//! The rows a write adds to and removes from one side of the type-membership
//! relation.
//!
//! A type selection walks the membership relation ([`crate::type_membership`]),
//! so every write that sets the types of an Entity's current state or of its
//! Temporal Evolution states them there too, in the same statement as the
//! identity row: a create lands one row per type, and a write that changes the
//! types moves only those that changed. Each statement hands over the rows it
//! wrote, with the type key arrays it had and has; these builders unnest them
//! into membership rows.
//!
//! One row serves both sides. Adding a type to a side sets that side's flag,
//! inserting the row when the other side does not hold it; removing a type
//! from a side clears the flag where the other side holds the type and deletes
//! the row where it does not, so no row is left belonging to neither.

use crate::type_membership::Memberships;
use andromeda_postgresql_schema::{current::EntityTypeMembers, type_membership::MembershipSide};
use andromeda_postgresql_seaquery::{
    literal::{false_literal, true_literal},
    pg_array::any,
    unnest::unnest_of,
};
use sea_query::{
    DeleteStatement,
    Expr,
    ExprTrait,
    Iden,
    InsertStatement,
    JoinType,
    OnConflict,
    Query,
    SelectStatement,
    SimpleExpr,
    UpdateStatement,
    error::Error as SeaQueryError,
};

/// The lateral that unnests one type-key array, one row per key.
#[derive(Iden, Clone, Copy)]
#[iden = "carried"]
struct Carried;

/// The identity row a membership row belongs to.
#[derive(Clone, Debug)]
pub struct Member {
    /// The row's surrogate key.
    pub key: SimpleExpr,
    /// The Entity's identifier.
    pub id: SimpleExpr,
}

/// The membership rows that leave one side of the Entities a write names,
/// as the two statements that move them: the rows the other side keeps lose
/// the flag, the others go.
#[derive(Debug)]
pub struct MembershipRemoval {
    /// Clears the side's flag on the rows the other side holds.
    pub cleared: UpdateStatement,
    /// Deletes the rows the other side does not hold.
    pub deleted: DeleteStatement,
}

/// The membership rows of the identity rows `rows` yields on the side
/// `memberships` names: one per key of `keys` for which `admitted` holds,
/// when given. A row the other side already holds gains this side's flag.
/// An inserted row carries `other_side` as the other side's flag: the
/// statement that writes both sides of an Entity at once states both.
///
/// # Errors
///
/// Returns [`SeaQueryError`] if the projection ever stops matching the column
/// list, which the fixed shape here makes unreachable.
pub fn membership_insert(
    memberships: Memberships<'_>,
    mut rows: SelectStatement,
    keys: SimpleExpr,
    member: Member,
    admitted: Option<SimpleExpr>,
    other_side: SimpleExpr,
) -> Result<InsertStatement, SeaQueryError> {
    let side = memberships.side;
    let (current, evolution) = match side {
        MembershipSide::Current => (true_literal(), other_side),
        MembershipSide::Evolution => (other_side, true_literal()),
    };
    unnest_keys(&mut rows, keys);
    rows.expr(member.key).expr(carried_type_key()).expr(member.id).expr(current).expr(evolution);
    if let Some(admitted) = admitted {
        rows.and_where(admitted);
    }
    let mut insert = Query::insert();
    insert
        .into_table(memberships.table())
        .columns([
            EntityTypeMembers::EntityKey,
            EntityTypeMembers::EntityTypeNameKey,
            EntityTypeMembers::Id,
            EntityTypeMembers::IsCurrentMember,
            EntityTypeMembers::IsEvolutionMember,
        ])
        .select_from(rows)?
        .on_conflict(
            OnConflict::columns([EntityTypeMembers::EntityTypeNameKey, EntityTypeMembers::Id])
                .value(side.flag(), true_literal())
                .to_owned(),
        );
    Ok(insert)
}

/// The membership rows to take off the side `memberships` names for the
/// identity rows `rows` yields: the keys of `dropped` that `kept` does not
/// hold, found by the membership key of the Entity `id` names.
#[must_use]
pub fn membership_removal(memberships: Memberships<'_>, mut rows: SelectStatement, dropped: SimpleExpr, kept: SimpleExpr, id: SimpleExpr) -> MembershipRemoval {
    let side = memberships.side;
    unnest_keys(&mut rows, dropped);
    rows.expr(carried_type_key()).expr(id).and_where(carried_among(kept).not());
    let addressed = || Expr::tuple([Expr::col(EntityTypeMembers::EntityTypeNameKey), Expr::col(EntityTypeMembers::Id)]);
    let other = || Expr::col(side.other().flag());
    MembershipRemoval {
        cleared: Query::update()
            .table(memberships.table())
            .value(side.flag(), false_literal())
            // clone: the rows address both the cleared and the deleted memberships.
            .and_where(addressed().in_subquery(rows.clone()))
            .and_where(other())
            .take(),
        deleted: Query::delete()
            .from_table(memberships.table())
            .and_where(addressed().in_subquery(rows))
            .and_where(other().not())
            .take(),
    }
}

/// Every membership row of the Entities whose keys `entity_keys` selects,
/// taken off the side `memberships` names: the side's whole departure, when
/// a core deletion (clause 5.6.6) removes the current state or a Temporal
/// Evolution deletion (clause 5.6.16) the Evolution, while the other side
/// stays. The rows are found by the Entity's key.
#[must_use]
pub fn side_departure(memberships: Memberships<'_>, entity_keys: SelectStatement) -> MembershipRemoval {
    let side = memberships.side;
    let departing = || Expr::col(EntityTypeMembers::EntityKey);
    let other = || Expr::col(side.other().flag());
    MembershipRemoval {
        cleared: Query::update()
            .table(memberships.table())
            .value(side.flag(), false_literal())
            // clone: the keys address both the cleared and the deleted memberships.
            .and_where(departing().in_subquery(entity_keys.clone()))
            .and_where(other())
            .take(),
        deleted: Query::delete()
            .from_table(memberships.table())
            .and_where(departing().in_subquery(entity_keys))
            .and_where(other().not())
            .take(),
    }
}

/// Whether the key the lateral unnested is one of `keys`: the admission of a
/// [`membership_insert`] that adds only the types a row did not hold, negated.
#[must_use]
pub fn carried_among(keys: SimpleExpr) -> SimpleExpr {
    carried_type_key().eq(any(keys))
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
        true_literal(),
    );
}

/// The key the lateral unnested: `carried.entity_type_name_key`.
fn carried_type_key() -> SimpleExpr {
    Expr::col((Carried, EntityTypeMembers::EntityTypeNameKey))
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        type_membership::Memberships,
        type_membership_moves::{Member, carried_among, membership_insert, membership_removal, side_departure},
    };
    use andromeda_postgresql_schema::type_membership::MembershipSide;
    use andromeda_postgresql_seaquery::literal::false_literal;
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, DynIden, Expr, ExprTrait, IntoIden, PostgresQueryBuilder, Query, SelectStatement, SimpleExpr};

    fn evolution(schema: Option<&DynIden>) -> Memberships<'_> {
        Memberships {
            side: MembershipSide::Evolution,
            schema,
        }
    }

    fn written() -> SelectStatement {
        Query::select().from(Alias::new("written")).take()
    }

    fn column(name: &str) -> SimpleExpr {
        Expr::col((Alias::new("written"), Alias::new(name)))
    }

    fn member() -> Member {
        Member {
            key: column("entity_key"),
            id: column("id"),
        }
    }

    /// A row the current state already holds gains the Evolution's flag
    /// rather than conflicting.
    #[test]
    fn an_insert_lands_one_membership_row_per_carried_type_on_its_side() {
        let sql = membership_insert(evolution(None), written(), column("new_types"), member(), None, false_literal())
            .expect("the membership insert builds")
            .to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            concat!(
                r#"INSERT INTO "entity_type_members" ("entity_key", "entity_type_name_key", "id", "is_current_member", "is_evolution_member") "#,
                r#"SELECT "written"."entity_key", "carried"."entity_type_name_key", "written"."id", FALSE, TRUE FROM "written" "#,
                r#"INNER JOIN LATERAL (SELECT UNNEST("written"."new_types") AS "entity_type_name_key") AS "carried" ON TRUE "#,
                r#"ON CONFLICT ("entity_type_name_key", "id") DO UPDATE SET "is_evolution_member" = TRUE"#
            )
        );
    }

    /// A create that also establishes the Evolution states the Evolution's
    /// flag beside the current one, so the row is written once.
    #[test]
    fn an_insert_states_the_other_sides_flag_it_is_given() {
        let current = Memberships {
            side: MembershipSide::Current,
            schema: None,
        };
        let sql = membership_insert(current, written(), column("new_types"), member(), None, column("established"))
            .expect("the membership insert builds")
            .to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(r#"SELECT "written"."entity_key", "carried"."entity_type_name_key", "written"."id", TRUE, "written"."established" FROM "written" "#),
            "{sql}"
        );
        assert!(sql.ends_with(r#"DO UPDATE SET "is_current_member" = TRUE"#), "{sql}");
    }

    #[test]
    fn an_admitted_insert_keeps_only_the_types_its_condition_admits() {
        let admitted = carried_among(column("old_types")).not();
        let sql = membership_insert(evolution(None), written(), column("new_types"), member(), Some(admitted), false_literal())
            .expect("the membership insert builds")
            .to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(r#"WHERE NOT "carried"."entity_type_name_key" = ANY("written"."old_types") ON CONFLICT"#),
            "{sql}"
        );
    }

    /// A type the current state drops stays a row while the Evolution holds
    /// it, and goes when it does not.
    #[test]
    fn a_removal_clears_the_flag_the_other_side_keeps_and_deletes_the_rest() {
        let current = Memberships {
            side: MembershipSide::Current,
            schema: None,
        };
        let removal = membership_removal(current, written(), column("old_types"), column("new_types"), column("id"));
        let addressed = concat!(
            r#"("entity_type_name_key", "id") IN "#,
            r#"(SELECT "carried"."entity_type_name_key", "written"."id" FROM "written" "#,
            r#"INNER JOIN LATERAL (SELECT UNNEST("written"."old_types") AS "entity_type_name_key") AS "carried" ON TRUE "#,
            r#"WHERE NOT "carried"."entity_type_name_key" = ANY("written"."new_types"))"#
        );
        assert_eq!(
            removal.cleared.to_string(PostgresQueryBuilder),
            format!(r#"UPDATE "entity_type_members" SET "is_current_member" = FALSE WHERE {addressed} AND "is_evolution_member""#)
        );
        assert_eq!(
            removal.deleted.to_string(PostgresQueryBuilder),
            format!(r#"DELETE FROM "entity_type_members" WHERE {addressed} AND (NOT "is_evolution_member")"#)
        );
    }

    /// A core deletion leaves the Evolution's rows standing without the
    /// current flag, found by the Entity's key.
    #[test]
    fn a_departing_side_leaves_the_rows_the_other_side_holds() {
        let current = Memberships {
            side: MembershipSide::Current,
            schema: None,
        };
        let keys = Query::select().column(Alias::new("entity_key")).from(Alias::new("doomed")).take();
        let removal = side_departure(current, keys);
        assert_eq!(
            removal.cleared.to_string(PostgresQueryBuilder),
            r#"UPDATE "entity_type_members" SET "is_current_member" = FALSE WHERE "entity_key" IN (SELECT "entity_key" FROM "doomed") AND "is_evolution_member""#
        );
        assert_eq!(
            removal.deleted.to_string(PostgresQueryBuilder),
            r#"DELETE FROM "entity_type_members" WHERE "entity_key" IN (SELECT "entity_key" FROM "doomed") AND (NOT "is_evolution_member")"#
        );
    }

    #[test]
    fn a_branch_moves_its_memberships_in_its_own_schema() {
        let schema = Alias::new("snap_5").into_iden();
        let added = membership_insert(evolution(Some(&schema)), written(), column("new_types"), member(), None, false_literal())
            .expect("the membership insert builds")
            .to_string(PostgresQueryBuilder);
        let removal = membership_removal(evolution(Some(&schema)), written(), column("old_types"), column("new_types"), column("id"));
        assert!(added.starts_with(r#"INSERT INTO "snap_5"."entity_type_members""#), "{added}");
        assert!(
            removal
                .cleared
                .to_string(PostgresQueryBuilder)
                .starts_with(r#"UPDATE "snap_5"."entity_type_members""#),
            "{removal:?}"
        );
        assert!(
            removal
                .deleted
                .to_string(PostgresQueryBuilder)
                .starts_with(r#"DELETE FROM "snap_5"."entity_type_members""#),
            "{removal:?}"
        );
    }
}
