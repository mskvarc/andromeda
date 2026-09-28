//! The no-op guard of the projection upsert: a stored row whose projection is
//! value-identical to the proposed one is not rewritten.

use crate::current_attribute_values::column_sets::REFRESHED_COLUMNS;
use andromeda_postgresql_schema::current::CurrentAttributeValues;
use andromeda_postgresql_seaquery::pg_operator::PgBinaryOperator;
use sea_query::{Expr, Iden, OnConflict, SimpleExpr};

/// The `excluded` pseudo-relation an `ON CONFLICT DO UPDATE` action reads the
/// proposed row from.
#[derive(Iden, Clone, Copy)]
#[iden = "excluded"]
pub(super) struct Excluded;

/// The no-op guard of the conflict action: the stored row's refreshed columns,
/// as one row value, are distinct from the proposed row's.
///
/// `IS DISTINCT FROM` over row values compares field by field with NULL
/// counting as a value, so a NULL typed column that stays NULL is equal, and a
/// value that moves into a different typed column is a difference on two
/// fields at once. Written over every refreshed column rather than a subset so
/// the guard and the `SET` list can never disagree about what "identical" means.
pub(super) fn stored_row_differs_from<T>(proposed_source: T) -> SimpleExpr
where
    T: Iden + Copy + Send + Sync + 'static,
{
    let stored = Expr::tuple(REFRESHED_COLUMNS.map(|column| Expr::col((CurrentAttributeValues::Table, column))));
    let proposed = Expr::tuple(REFRESHED_COLUMNS.map(|column| Expr::col((proposed_source, column))));
    stored.distinct_from(proposed)
}

/// The natural-grain conflict action retained on the insert solely for a
/// concurrent transaction that created the same missing grain in between.
pub(super) fn concurrent_insert_conflict() -> OnConflict {
    OnConflict::columns([
        CurrentAttributeValues::EntityKey,
        CurrentAttributeValues::AttributeNameKey,
        CurrentAttributeValues::DatasetId,
    ])
    .update_columns(REFRESHED_COLUMNS)
    .action_and_where(stored_row_differs_from(Excluded))
    .to_owned()
}
