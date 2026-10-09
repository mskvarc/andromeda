//! The `instances` staging every statement writing `current_attributes` rows
//! reads them from.
//!
//! The rows are transposed into one array per column and expanded with
//! `UNNEST` under the alias `r`, then projected once, in the common table
//! expression `instances`, into the types the columns store: the EWKB of a
//! geometry is read and validated and the `objectType` keys are cast from
//! their array literal. Each row's elements travel in a `sub_attribute_tree`,
//! because PostgreSQL arrays are rectangular; an `UNNEST` of several arrays
//! expands a composite element into its fields, so the staged column is the
//! tree's one field, the `sub_attribute[]` itself. Every row carries its
//! Entity's identifier, which is how a statement pairs it with the Entity row
//! it writes: never by position.

use andromeda_postgresql_schema::current::{CurrentAttributes, Entities};
use andromeda_postgresql_seaquery::{
    cast::cast,
    pg_type::PostgresType,
    placeholder::external_placeholder,
    postgis::validated_geom_from_ewkb,
    unnest::{UnnestColumn, apply_column_alias, unnest_call},
};
use sea_query::{CommonTableExpression, Expr, Iden, Query, SimpleExpr};

/// The row staging alias, `r`.
///
/// Spelled nowhere else in a statement, so the single `AS "r"` its table
/// source emits is the one [`apply_instances_alias`] rewrites.
#[derive(Iden, Clone, Copy)]
#[iden = "r"]
struct Staged;

/// The common table expression, `instances`, every statement writing rows
/// reads them from.
#[derive(Iden, Clone, Copy)]
#[iden = "instances"]
pub(crate) struct Instances;

/// Every `current_attributes` column a row is written with, the Entity's key
/// aside, in bind order. The staging carries the same columns, after the
/// Entity's identifier.
pub(crate) const ROW_COLUMNS: [CurrentAttributes; 26] = [
    CurrentAttributes::AttributeNameKey,
    CurrentAttributes::DatasetId,
    CurrentAttributes::Ordinal,
    CurrentAttributes::AttributeKind,
    CurrentAttributes::ValueKind,
    CurrentAttributes::ValueNumber,
    CurrentAttributes::ValueString,
    CurrentAttributes::ValueLongText,
    CurrentAttributes::ValueBoolean,
    CurrentAttributes::ValueDateTime,
    CurrentAttributes::ValueGeometry,
    CurrentAttributes::ValueGeometryMembers,
    CurrentAttributes::ValueObject,
    CurrentAttributes::ValueUri,
    CurrentAttributes::ValueLongUri,
    CurrentAttributes::ValueDatatypeKey,
    CurrentAttributes::ValueLanguage,
    CurrentAttributes::ValueTypeKey,
    CurrentAttributes::ObjectTypeKeys,
    CurrentAttributes::UnitCodeId,
    CurrentAttributes::ObservedAt,
    CurrentAttributes::CreatedAt,
    CurrentAttributes::ModifiedAt,
    CurrentAttributes::ExpiresAt,
    CurrentAttributes::ProofSeals,
    CurrentAttributes::SubAttributes,
];

/// The array type each staged column binds as: the column's own type, but
/// EWKB for a geometry, an array literal for the `objectType` keys and the
/// carrier composite for the elements.
const fn staged_type(column: CurrentAttributes) -> PostgresType {
    match column {
        CurrentAttributes::AttributeNameKey | CurrentAttributes::Ordinal | CurrentAttributes::ValueDatatypeKey | CurrentAttributes::ValueTypeKey => {
            PostgresType::IntegerArray
        }
        CurrentAttributes::AttributeKind | CurrentAttributes::ValueKind | CurrentAttributes::UnitCodeId => PostgresType::SmallintArray,
        CurrentAttributes::ValueNumber => PostgresType::NumericArray,
        CurrentAttributes::ValueBoolean => PostgresType::BooleanArray,
        CurrentAttributes::ValueDateTime
        | CurrentAttributes::ObservedAt
        | CurrentAttributes::CreatedAt
        | CurrentAttributes::ModifiedAt
        | CurrentAttributes::ExpiresAt => PostgresType::TimestamptzArray,
        CurrentAttributes::ValueGeometry => PostgresType::ByteaArray,
        CurrentAttributes::ValueGeometryMembers | CurrentAttributes::ValueObject | CurrentAttributes::ProofSeals => PostgresType::JsonbArray,
        CurrentAttributes::SubAttributes => PostgresType::SubAttributeTreeArray,
        CurrentAttributes::Table
        | CurrentAttributes::EntityKey
        | CurrentAttributes::DatasetId
        | CurrentAttributes::ValueString
        | CurrentAttributes::ValueLongText
        | CurrentAttributes::ValueUri
        | CurrentAttributes::ValueLongUri
        | CurrentAttributes::ValueLanguage
        | CurrentAttributes::ObjectTypeKeys => PostgresType::TextArray,
    }
}

/// The staging columns, `AS r(id, …)`, in bind order.
fn staged_columns() -> Vec<UnnestColumn> {
    let mut columns = vec![UnnestColumn::new(Entities::Id, PostgresType::TextArray)];
    columns.extend(ROW_COLUMNS.map(|column| UnnestColumn::new(column, staged_type(column))));
    columns
}

/// The stored value of one staged column, `r.<column>` as the column stores
/// it.
fn projected(column: CurrentAttributes) -> SimpleExpr {
    let staged: SimpleExpr = Expr::col((Staged, column));
    match column {
        CurrentAttributes::ValueGeometry => validated_geom_from_ewkb(staged),
        CurrentAttributes::ObjectTypeKeys => cast(staged, PostgresType::IntegerArray),
        CurrentAttributes::Table
        | CurrentAttributes::EntityKey
        | CurrentAttributes::CreatedAt
        | CurrentAttributes::ModifiedAt
        | CurrentAttributes::AttributeNameKey
        | CurrentAttributes::Ordinal
        | CurrentAttributes::AttributeKind
        | CurrentAttributes::ValueKind
        | CurrentAttributes::ValueDateTime
        | CurrentAttributes::ObservedAt
        | CurrentAttributes::ExpiresAt
        | CurrentAttributes::ValueDatatypeKey
        | CurrentAttributes::ValueTypeKey
        | CurrentAttributes::UnitCodeId
        | CurrentAttributes::ValueBoolean
        | CurrentAttributes::DatasetId
        | CurrentAttributes::ValueNumber
        | CurrentAttributes::ValueString
        | CurrentAttributes::ValueLanguage
        | CurrentAttributes::ValueGeometryMembers
        | CurrentAttributes::ValueObject
        | CurrentAttributes::ValueUri
        | CurrentAttributes::ValueLongText
        | CurrentAttributes::ValueLongUri
        | CurrentAttributes::ProofSeals
        | CurrentAttributes::SubAttributes => staged,
    }
}

/// The `instances` CTE over the staging binds from placeholder `first` on:
/// `instances AS (SELECT r.id, r.attribute_name_key, … FROM UNNEST($first::text[], …) AS r(id, …))`.
///
/// Its columns carry the names of the `current_attributes` columns they land
/// in, beside `id`.
pub(crate) fn instances_cte(first: usize) -> CommonTableExpression {
    let columns = staged_columns();
    let source = unnest_call(columns.iter().enumerate().map(|(index, column)| (column, external_placeholder(first + index))));
    let mut select = Query::select();
    select.expr_as(Expr::col((Staged, Entities::Id)), Entities::Id);
    for column in ROW_COLUMNS {
        select.expr_as(projected(column), column);
    }
    select.from_function(source, Staged);
    CommonTableExpression::new().table_name(Instances).query(select.take()).to_owned()
}

/// Rewrite the row staging's `AS "r"` into `AS r(id, …)`.
pub(crate) fn apply_instances_alias(sql: String) -> String {
    apply_column_alias(sql, &Staged, &staged_columns())
}

#[cfg(test)]
mod tests {
    use crate::current_attribute::staging::{apply_instances_alias, instances_cte, staged_columns};
    use sea_query::{PostgresQueryBuilder, Query, WithClause};

    /// One bind per staged column: the identifier, then every row column.
    #[test]
    fn the_staging_names_one_column_per_bind() {
        let mut with_clause = WithClause::new();
        with_clause.cte(instances_cte(7));
        let sql = apply_instances_alias(with_clause.query(Query::select().take()).to_string(PostgresQueryBuilder));
        pretty_assertions::assert_eq!(staged_columns().len(), 27);
        assert!(sql.contains("UNNEST($7::text[], $8::integer[]"), "{sql}");
        assert!(sql.contains("$33::sub_attribute_tree[]) AS r(id, attribute_name_key,"), "{sql}");
        assert!(
            sql.contains(r#"ST_MakeValid(ST_GeomFromEWKB("r"."value_geometry")) AS "value_geometry""#),
            "{sql}"
        );
    }
}
