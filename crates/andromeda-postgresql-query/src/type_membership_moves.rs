//! The rows a write adds to and removes from a type-membership relation.
//!
//! A type selection walks the membership relation ([`crate::type_membership`]),
//! so every write that sets the types of an Entity or a Temporal Evolution
//! states them there too, in the same statement as the catalogue row: a create
//! lands one row per type, and a write that changes the types moves only those
//! that changed. Each statement hands over the rows it wrote, with the type key
//! arrays it had and has; these builders unnest them into membership rows.

use crate::type_membership::Memberships;
use andromeda_postgresql_seaquery::{literal::true_literal, pg_array::any, unnest::unnest_of};
use sea_query::{DeleteStatement, Expr, ExprTrait, Iden, InsertStatement, JoinType, Query, SelectStatement, SimpleExpr, error::Error as SeaQueryError};

/// The lateral that unnests one type-key array, one row per key.
#[derive(Iden, Clone, Copy)]
#[iden = "carried"]
struct Carried;

/// The catalogue row a membership row belongs to.
#[derive(Clone, Debug)]
pub struct Member {
    /// The row's surrogate key.
    pub key: SimpleExpr,
    /// The Entity's identifier.
    pub id: SimpleExpr,
}

/// The membership rows of the catalogue rows `rows` yields: one per key of
/// `keys` for which `admitted` holds, when given.
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
) -> Result<InsertStatement, SeaQueryError> {
    let relation = memberships.relation;
    unnest_keys(&mut rows, keys, memberships);
    rows.expr(member.key).expr(carried_type_key(memberships)).expr(member.id);
    if let Some(admitted) = admitted {
        rows.and_where(admitted);
    }
    let mut insert = Query::insert();
    insert
        .into_table(memberships.table())
        .columns([relation.key(), relation.type_key(), relation.id()])
        .select_from(rows)?;
    Ok(insert)
}

/// The membership rows to remove for the catalogue rows `rows` yields: the
/// keys of `dropped` that `kept` does not hold, found by the membership key
/// of the Entity `id` names.
#[must_use]
pub fn membership_delete(memberships: Memberships<'_>, mut rows: SelectStatement, dropped: SimpleExpr, kept: SimpleExpr, id: SimpleExpr) -> DeleteStatement {
    let relation = memberships.relation;
    unnest_keys(&mut rows, dropped, memberships);
    rows.expr(carried_type_key(memberships))
        .expr(id)
        .and_where(carried_among(kept, memberships).not());
    Query::delete()
        .from_table(memberships.table())
        .and_where(Expr::tuple([Expr::col(relation.type_key()), Expr::col(relation.id())]).in_subquery(rows))
        .take()
}

/// Whether the key the lateral unnested is one of `keys`: the admission of a
/// [`membership_insert`] that adds only the types a row did not hold, negated.
#[must_use]
pub fn carried_among(keys: SimpleExpr, memberships: Memberships<'_>) -> SimpleExpr {
    carried_type_key(memberships).eq(any(keys))
}

/// Join one row per key of `keys` to `rows`, as
/// `INNER JOIN LATERAL (SELECT UNNEST(keys) AS entity_type_name_key) AS carried ON TRUE`.
fn unnest_keys(rows: &mut SelectStatement, keys: SimpleExpr, memberships: Memberships<'_>) {
    rows.join_lateral(
        JoinType::InnerJoin,
        Query::select()
            .expr_as(SimpleExpr::from(unnest_of(keys)), memberships.relation.type_key())
            .take(),
        Carried,
        true_literal(),
    );
}

/// The key the lateral unnested: `carried.entity_type_name_key`.
fn carried_type_key(memberships: Memberships<'_>) -> SimpleExpr {
    Expr::col((Carried, memberships.relation.type_key()))
}

#[cfg(test)]
mod tests {
    use crate::{
        type_membership::Memberships,
        type_membership_moves::{Member, carried_among, membership_delete, membership_insert},
    };
    use andromeda_postgresql_schema::type_membership::TypeMembershipRelation;
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Expr, ExprTrait, IntoIden, PostgresQueryBuilder, Query};

    fn temporal(schema: Option<&sea_query::DynIden>) -> Memberships<'_> {
        Memberships {
            relation: TypeMembershipRelation::Temporal,
            schema,
        }
    }

    fn written() -> sea_query::SelectStatement {
        Query::select().from(Alias::new("written")).take()
    }

    fn column(name: &str) -> sea_query::SimpleExpr {
        Expr::col((Alias::new("written"), Alias::new(name)))
    }

    fn member() -> Member {
        Member {
            key: column("temporal_entity_key"),
            id: column("id"),
        }
    }

    #[test]
    fn an_insert_lands_one_membership_row_per_carried_type() {
        let sql = membership_insert(temporal(None), written(), column("new_types"), member(), None)
            .expect("the membership insert builds")
            .to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            concat!(
                r#"INSERT INTO "temporal_entity_type_members" ("temporal_entity_key", "entity_type_name_key", "id") "#,
                r#"SELECT "written"."temporal_entity_key", "carried"."entity_type_name_key", "written"."id" FROM "written" "#,
                r#"INNER JOIN LATERAL (SELECT UNNEST("written"."new_types") AS "entity_type_name_key") AS "carried" ON TRUE"#
            )
        );
    }

    #[test]
    fn an_admitted_insert_keeps_only_the_types_its_condition_admits() {
        let admitted = carried_among(column("old_types"), temporal(None)).not();
        let sql = membership_insert(temporal(None), written(), column("new_types"), member(), Some(admitted))
            .expect("the membership insert builds")
            .to_string(PostgresQueryBuilder);
        assert!(
            sql.ends_with(r#"WHERE NOT "carried"."entity_type_name_key" = ANY("written"."old_types")"#),
            "{sql}"
        );
    }

    #[test]
    fn a_delete_removes_by_the_membership_key_the_dropped_types_no_longer_kept() {
        let sql = membership_delete(temporal(None), written(), column("old_types"), column("new_types"), column("id")).to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            concat!(
                r#"DELETE FROM "temporal_entity_type_members" WHERE ("entity_type_name_key", "id") IN "#,
                r#"(SELECT "carried"."entity_type_name_key", "written"."id" FROM "written" "#,
                r#"INNER JOIN LATERAL (SELECT UNNEST("written"."old_types") AS "entity_type_name_key") AS "carried" ON TRUE "#,
                r#"WHERE NOT "carried"."entity_type_name_key" = ANY("written"."new_types"))"#
            )
        );
    }

    #[test]
    fn a_branch_moves_its_memberships_in_its_own_schema() {
        let schema = Alias::new("snap_5").into_iden();
        let added = membership_insert(temporal(Some(&schema)), written(), column("new_types"), member(), None)
            .expect("the membership insert builds")
            .to_string(PostgresQueryBuilder);
        let dropped =
            membership_delete(temporal(Some(&schema)), written(), column("old_types"), column("new_types"), column("id")).to_string(PostgresQueryBuilder);
        assert!(added.starts_with(r#"INSERT INTO "snap_5"."temporal_entity_type_members""#), "{added}");
        assert!(dropped.starts_with(r#"DELETE FROM "snap_5"."temporal_entity_type_members""#), "{dropped}");
    }
}
