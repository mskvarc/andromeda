//! Reading interned keys back as the names the dictionaries hold.

use andromeda_postgresql_schema::name_dictionary::{AttributeNames, EntityTypeNames};
use sea_query::{Expr, ExprTrait, Iden, JoinType, SelectStatement, SimpleExpr};

/// The `entity_type_names` row a type key is read back through, `n`.
#[derive(Iden, Clone, Copy)]
#[iden = "n"]
pub(crate) struct TypeName;

/// The `attribute_names` row an attribute key is read back through, `an`.
#[derive(Iden, Clone, Copy)]
#[iden = "an"]
pub(crate) struct AttributeNameRow;

/// `JOIN entity_type_names n ON n.entity_type_name_key = <key>`.
pub(crate) fn join_type_name(statement: &mut SelectStatement, join: JoinType, key: SimpleExpr) {
    statement.join_as(
        join,
        EntityTypeNames::Table,
        TypeName,
        Expr::col((TypeName, EntityTypeNames::EntityTypeNameKey)).eq(key),
    );
}

/// `JOIN attribute_names an ON an.attribute_name_key = <key>`.
pub(crate) fn join_attribute_name(statement: &mut SelectStatement, join: JoinType, key: SimpleExpr) {
    statement.join_as(
        join,
        AttributeNames::Table,
        AttributeNameRow,
        Expr::col((AttributeNameRow, AttributeNames::AttributeNameKey)).eq(key),
    );
}
