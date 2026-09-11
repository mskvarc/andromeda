//! The row source and the keyset guard every discovery aggregate is built on.
//!
//! A discovery aggregate reads two rows per entity: the search row carries the
//! interned type keys, the document row carries the attribute members. Both the
//! join between them and the cursor guard that pages the groups are written
//! once here, so the type aggregates and the attribute aggregates cannot drift
//! apart on either.

use andromeda_context_space::context_space_key::ContextSpaceKey;
use andromeda_postgresql_schema::current::{EntityDocuments, EntitySearch};
use andromeda_postgresql_seaquery::{cast::cast, jsonb::jsonb_typeof, pg_operator::PgBinaryOperator, pg_type::PostgresType};
use sea_query::{CaseStatement, Cond, Expr, ExprTrait, Iden, JoinType, SelectStatement, SimpleExpr};

/// The `entity_search` row an aggregate reads its type keys from, aliased `s`.
#[derive(Iden, Clone, Copy)]
#[iden = "s"]
pub(super) struct S;

/// The `entity_documents` row an aggregate reads its payload from, aliased `d`.
#[derive(Iden, Clone, Copy)]
#[iden = "d"]
pub(super) struct D;

/// The attribute key an aggregate groups on.
#[derive(Iden, Clone, Copy)]
#[iden = "attr_key"]
pub(super) struct AttrKey;

/// The attribute container of a payload: `payload -> 0` when that node is a JSON
/// object, and the payload root otherwise.
///
/// An entity's attributes live under the first element of its expanded payload
/// array, except for the fragment shapes whose root is already the attribute
/// object; the `CASE` reads whichever holds the members. `payload` is passed as
/// a fresh expression each time so the node can appear in the test, the `THEN`
/// and the `ELSE` without sharing an owned value.
pub(super) fn attribute_container(payload: &dyn Fn() -> SimpleExpr) -> SimpleExpr {
    let node = || payload().json_get(Expr::val(0));
    CaseStatement::new()
        .case(Expr::expr(jsonb_typeof(node())).eq("object"), node())
        .finally(payload())
        .into()
}

/// `FROM entity_search s JOIN entity_documents d ON d.entity_key = s.entity_key`,
/// the source every payload-walking aggregate reads: the type keys from the
/// search row, the attribute members from the document row.
pub(super) fn search_with_documents(statement: &mut SelectStatement, context_space_key: ContextSpaceKey) -> &mut SelectStatement {
    statement
        .from_as(EntitySearch::Table, S)
        .join_as(
            JoinType::Join,
            EntityDocuments::Table,
            D,
            Expr::col((D, EntityDocuments::EntityKey))
                .equals((S, EntitySearch::EntityKey))
                .and(Expr::col((D, EntityDocuments::ContextSpaceKey)).equals((S, EntitySearch::ContextSpaceKey))),
        )
        .and_where(Expr::col((S, EntitySearch::ContextSpaceKey)).eq(context_space_key.as_i64()))
}

/// The keyset guard `$after::text IS NULL OR key > $after`: an absent cursor
/// admits every group, a present one only the groups past it.
pub(super) fn keyset_after(key: SimpleExpr, after: Option<&str>) -> Cond {
    let after = after.map(str::to_owned);
    Cond::any()
        .add(Expr::expr(cast(Expr::val(after.clone()), PostgresType::Text)).is_null())
        .add(Expr::expr(key).gt(Expr::val(after)))
}
