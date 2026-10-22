//! Rows written straight into the store, bypassing the broker.
//!
//! A schema constraint is only proven by a writer that does not go through the
//! Rust code enforcing the same rule first. These statements give such a
//! writer the parent rows an attribute row needs, inside a transaction the
//! caller rolls back.

use andromeda_postgresql_schema::{
    current::{CurrentAttributes, Entities, EntitySearch, EntityTypeMembers},
    name_dictionary::{AttributeNames, EntityTypeNames},
};
use andromeda_postgresql_seaquery::{clock::now, statement::Statement};
use sea_query::{Alias, CommonTableExpression, Expr, InsertStatement, OnConflict, Query, SimpleExpr, WithClause};
use sqlx::PgConnection;

/// The common table expression holding the Entity row just inserted.
const INSERTED: &str = "ins";

/// The common table expression holding the type membership just inserted.
const MEMBERSHIP: &str = "mbr";

/// Insert a bare current Entity of one expanded type, with its search row and
/// its type membership, and return its surrogate key.
pub async fn insert_typed_entity(connection: &mut PgConnection, entity_id: &str, type_iri: &str) -> i64 {
    let type_key: i32 = Query::insert()
        .into_table(EntityTypeNames::Table)
        .columns([EntityTypeNames::Name])
        .values([Expr::val(type_iri)])
        .expect("one value per column")
        .on_conflict(OnConflict::column(EntityTypeNames::Name).update_column(EntityTypeNames::Name).to_owned())
        .returning_col(EntityTypeNames::EntityTypeNameKey)
        .to_query_scalar()
        .fetch_one(&mut *connection)
        .await
        .expect("type dictionary insert must succeed");
    let entity = Query::insert()
        .into_table(Entities::Table)
        .columns([Entities::Id])
        .values([Expr::val(entity_id)])
        .expect("one value per column")
        .returning(Query::returning().columns([Entities::EntityKey, Entities::Id]))
        .to_owned();
    let membership = Query::insert()
        .into_table(EntityTypeMembers::Table)
        .columns([EntityTypeMembers::EntityKey, EntityTypeMembers::EntityTypeNameKey, EntityTypeMembers::Id])
        .select_from(
            Query::select()
                .column(Entities::EntityKey)
                .expr(Expr::val(type_key))
                .column(Entities::Id)
                .from(Alias::new(INSERTED))
                .to_owned(),
        )
        .expect("the projection matches the column list")
        .to_owned();
    let search = Query::insert()
        .into_table(EntitySearch::Table)
        .columns([
            EntitySearch::EntityKey,
            EntitySearch::EntityTypeNameKeys,
            EntitySearch::AttributeNameKeys,
            EntitySearch::AttributeInstanceNameKeys,
            EntitySearch::AttributeInstanceKinds,
        ])
        .select_from(
            Query::select()
                .column(Entities::EntityKey)
                .expr(Expr::val(vec![type_key]))
                .expr(Expr::val(Vec::<i32>::new()))
                .expr(Expr::val(Vec::<i32>::new()))
                .expr(Expr::val(Vec::<i16>::new()))
                .from(Alias::new(INSERTED))
                .to_owned(),
        )
        .expect("the projection matches the column list")
        .returning_col(EntitySearch::EntityKey)
        .to_owned();
    WithClause::new()
        .cte(CommonTableExpression::new().table_name(Alias::new(INSERTED)).query(entity).to_owned())
        .cte(CommonTableExpression::new().table_name(Alias::new(MEMBERSHIP)).query(membership).to_owned())
        .to_owned()
        .query(search)
        .to_query_scalar()
        .fetch_one(&mut *connection)
        .await
        .expect("entity insert must succeed")
}

/// Register one expanded attribute IRI in the attribute-name dictionary and
/// return its key, reusing the entry when it already exists.
pub async fn insert_attribute_name(connection: &mut PgConnection, attribute_iri: &str) -> i32 {
    Query::insert()
        .into_table(AttributeNames::Table)
        .columns([AttributeNames::Name])
        .values([Expr::val(attribute_iri)])
        .expect("one value per column")
        .on_conflict(OnConflict::column(AttributeNames::Name).update_column(AttributeNames::Name).to_owned())
        .returning_col(AttributeNames::AttributeNameKey)
        .to_query_scalar()
        .fetch_one(connection)
        .await
        .expect("the attribute dictionary must be writable")
}

/// A scalar attribute row for `entity_key` under the name `name_key`, written
/// with `value` in `value_column` and the `value_kind` that column belongs to.
pub fn current_attribute_insert(
    entity_key: i64,
    name_key: i32,
    ordinal: i32,
    value_kind: i16,
    value_column: CurrentAttributes,
    value: SimpleExpr,
) -> InsertStatement {
    Query::insert()
        .into_table(CurrentAttributes::Table)
        .columns([
            CurrentAttributes::EntityKey,
            CurrentAttributes::AttributeNameKey,
            CurrentAttributes::Ordinal,
            CurrentAttributes::AttributeKind,
            CurrentAttributes::ValueKind,
            value_column,
            CurrentAttributes::CreatedAt,
            CurrentAttributes::ModifiedAt,
        ])
        .values([
            Expr::val(entity_key),
            Expr::val(name_key),
            Expr::val(ordinal),
            Expr::val(1_i16),
            Expr::val(value_kind),
            value,
            now(),
            now(),
        ])
        .expect("one value per column")
        .to_owned()
}
