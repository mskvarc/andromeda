//! The update-first half of the projection upsert: every input whose grain
//! already exists is locked, updated in place by `ctid` when it differs, and
//! kept out of the insert.

use crate::current_attribute_values::upsert::{
    column_sets::{PROJECTION_COLUMNS, REFRESHED_COLUMNS},
    no_op_guard::stored_row_differs_from,
    staging::{Input, InputId},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::current::CurrentAttributeValues;
use andromeda_postgresql_seaquery::pg_operator::PgBinaryOperator;
use sea_query::{CommonTableExpression, Condition, Expr, ExprTrait, Iden, LockType, Query, SelectStatement, SimpleExpr};

/// Input rows whose projection grain already exists, together with its TID.
#[derive(Iden, Clone, Copy)]
#[iden = "matched"]
pub(super) struct Matched;

/// Existing projection rows changed by the batch.
#[derive(Iden, Clone, Copy)]
#[iden = "updated"]
pub(super) struct Updated;

/// The physical identifier of a matched projection row.
#[derive(Iden, Clone, Copy)]
#[iden = "target_tid"]
pub(super) struct TargetTid;

/// PostgreSQL's physical tuple-identifier system column.
#[derive(Iden, Clone, Copy)]
#[iden = "ctid"]
pub(super) struct Ctid;

/// The `matched` common table expression: input rows whose grain already exists,
/// locked, each carrying the physical tuple ID of the stored row.
///
/// The join uses NULL-safe equality for `datasetId` because absent dataset IDs
/// identify the single default attribute instance. Locking here is what lets the
/// update stage change rows by `ctid` without racing a concurrent writer.
pub(super) fn matched_cte(target: &ContextTarget) -> CommonTableExpression {
    let mut select = Query::select();
    select
        .columns(PROJECTION_COLUMNS.map(|column| (Input, column)))
        .column((Input, InputId))
        .expr_as(Expr::col((CurrentAttributeValues::Table, Ctid)), TargetTid)
        .from(Input)
        .inner_join(
            table_of(target, CurrentAttributeValues::Table),
            Condition::all()
                .add(Expr::col((CurrentAttributeValues::Table, CurrentAttributeValues::EntityKey)).equals((Input, CurrentAttributeValues::EntityKey)))
                .add(
                    Expr::col((CurrentAttributeValues::Table, CurrentAttributeValues::AttributeNameId))
                        .equals((Input, CurrentAttributeValues::AttributeNameId)),
                )
                .add(
                    SimpleExpr::from(Expr::col((CurrentAttributeValues::Table, CurrentAttributeValues::DatasetId)))
                        .not_distinct_from(Expr::col((Input, CurrentAttributeValues::DatasetId))),
                ),
        )
        .lock_with_tables(LockType::Update, [CurrentAttributeValues::Table]);
    let select = select;

    CommonTableExpression::new()
        .table_name(Matched)
        .columns(PROJECTION_COLUMNS)
        .column(InputId)
        .column(TargetTid)
        .query(select)
        .materialized(true)
        .to_owned()
}

/// The `updated` common table expression: the direct update of every matched row
/// whose refreshed columns differ, addressed by its physical tuple ID.
pub(super) fn updated_cte(target: &ContextTarget) -> CommonTableExpression {
    let mut update = Query::update();
    update
        .table(table_of(target, CurrentAttributeValues::Table))
        .values(REFRESHED_COLUMNS.map(|column| (column, Expr::col((Matched, column)))))
        .from(Matched)
        .and_where(Expr::col((CurrentAttributeValues::Table, Ctid)).equals((Matched, TargetTid)))
        .and_where(stored_row_differs_from(Matched))
        .returning_col((CurrentAttributeValues::Table, CurrentAttributeValues::EntityKey));
    let update = update;

    CommonTableExpression::new().table_name(Updated).query(update).to_owned()
}

/// The rows the final insert writes: every input the match stage did not claim,
/// including inputs whose stored row was value-identical and so left unchanged.
pub(super) fn unmatched_input_select() -> SelectStatement {
    let matched_input = Query::select()
        .expr(Expr::val(1))
        .from(Matched)
        .and_where(Expr::col((Matched, InputId)).equals((Input, InputId)))
        .to_owned();

    Query::select()
        .columns(PROJECTION_COLUMNS.map(|column| (Input, column)))
        .from(Input)
        .and_where(Expr::exists(matched_input).not())
        .to_owned()
}
