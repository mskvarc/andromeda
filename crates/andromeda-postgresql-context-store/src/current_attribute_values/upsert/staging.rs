//! The staging half of the projection upsert: the batch's parallel arrays
//! unnested under the `d` alias, deduplicated to the clause 4.5.5 grain and
//! with their geometries validated.

use crate::current_attribute_values::upsert::column_sets::PROJECTION_COLUMNS;
use andromeda_postgresql_schema::current::CurrentAttributeValues;
use andromeda_postgresql_seaquery::{
    placeholder::external_placeholder,
    postgis::validated_geom_from_ewkb,
    unnest::{UnnestColumn, unnest_call},
};
use sea_query::{CaseStatement, CommonTableExpression, Expr, ExprTrait, Iden, Keyword, Order, Query, SimpleExpr};

/// The `UNNEST` set-write's staging alias, `d`.
///
/// The rows of the parallel arrays are read under this alias before the insert
/// sees them; it is spelled nowhere else in the statement, so the single
/// `AS "d"` the table source emits is the one [`apply_with_ordinality`] rewrites.
#[derive(Iden, Clone, Copy)]
#[iden = "d"]
pub(super) struct Staging;

/// The row-position column `WITH ORDINALITY` appends to the staging alias.
#[derive(Iden, Clone, Copy)]
#[iden = "ordinality"]
pub(super) struct Ordinality;

/// The deduplicated and geometry-validated batch rows.
#[derive(Iden, Clone, Copy)]
#[iden = "input"]
pub(super) struct Input;

/// The source row's position in the caller's batch.
#[derive(Iden, Clone, Copy)]
#[iden = "input_id"]
pub(super) struct InputId;

/// A staging column reference, `d.<column>`.
pub(super) fn staged(column: CurrentAttributeValues) -> SimpleExpr {
    Expr::col((Staging, column))
}

/// The `input` common table expression: the batch, deduplicated to the clause
/// 4.5.5 grain and with its geometries validated.
///
/// `DISTINCT ON` collapses repeats of the grain before the insert sees them.
/// Without it a batch naming one instance twice would abort with "cannot affect
/// row a second time", since `ON CONFLICT DO UPDATE` refuses to touch a row
/// twice in one statement. The ordinality the `UNNEST` carries is the tie-break:
/// the last position in the batch wins, which is the order the caller's
/// operations were applied to the entity payload in.
///
/// The `value_geometry` column is wrapped as
/// `CASE WHEN d.value_geometry IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB(d.value_geometry)) ELSE NULL END`:
/// the EWKB already carries its SRID, so no re-SRID is applied, and a NULL EWKB
/// projects to a NULL geometry rather than a call on a NULL argument.
pub(super) fn input_cte(columns: &[UnnestColumn]) -> CommonTableExpression {
    let call = unnest_call(columns.iter().enumerate().map(|(index, column)| (column, external_placeholder(index + 1))));

    let geometry: SimpleExpr = CaseStatement::new()
        .case(
            Expr::col((Staging, CurrentAttributeValues::ValueGeometry)).is_not_null(),
            validated_geom_from_ewkb(staged(CurrentAttributeValues::ValueGeometry)),
        )
        .finally(Keyword::Null)
        .into();

    let mut select = Query::select();
    select
        .distinct_on([
            (Staging, CurrentAttributeValues::EntityKey),
            (Staging, CurrentAttributeValues::AttributeNameKey),
            (Staging, CurrentAttributeValues::DatasetId),
        ])
        .expr(staged(CurrentAttributeValues::EntityKey))
        .expr(staged(CurrentAttributeValues::AttributeNameKey))
        .expr(staged(CurrentAttributeValues::AttributeKind))
        .expr(staged(CurrentAttributeValues::ValueKind))
        .expr(staged(CurrentAttributeValues::ValueDateTime))
        .expr(staged(CurrentAttributeValues::UnitCodeId))
        .expr(staged(CurrentAttributeValues::ValueBoolean))
        .expr(staged(CurrentAttributeValues::DatasetId))
        .expr(staged(CurrentAttributeValues::ValueNumber))
        .expr(staged(CurrentAttributeValues::ValueString))
        .expr(geometry)
        .expr(staged(CurrentAttributeValues::ValueObject))
        .expr(staged(CurrentAttributeValues::ValueUri))
        .expr(staged(CurrentAttributeValues::ValueLongText))
        .expr(Expr::col((Staging, Ordinality)))
        .from_function(call, Staging)
        .order_by((Staging, CurrentAttributeValues::EntityKey), Order::Asc)
        .order_by((Staging, CurrentAttributeValues::AttributeNameKey), Order::Asc)
        .order_by((Staging, CurrentAttributeValues::DatasetId), Order::Asc)
        .order_by((Staging, Ordinality), Order::Desc);
    let select = select;

    CommonTableExpression::new()
        .table_name(Input)
        .columns(PROJECTION_COLUMNS)
        .column(InputId)
        .query(select)
        .materialized(true)
        .to_owned()
}
