//! The column lists every stage of the projection upsert shares.

use andromeda_postgresql_schema::current::CurrentAttributeValues;
use andromeda_postgresql_seaquery::{pg_type::PostgresType, unnest::UnnestColumn};

/// The columns the upsert refreshes on conflict, in table order: everything the
/// row carries except the three grain columns the conflict target names.
///
/// This is also the exact column set the no-op guard compares, so a projected
/// row and its stored predecessor are value-identical when and only when every
/// column here is; the grain columns are equal by construction on a conflict.
/// `value_long_text` is among them, so a String moving between its two columns
/// is a difference on two fields at once.
pub(super) const REFRESHED_COLUMNS: [CurrentAttributeValues; 11] = [
    CurrentAttributeValues::AttributeKind,
    CurrentAttributeValues::ValueKind,
    CurrentAttributeValues::ValueDatetime,
    CurrentAttributeValues::UnitCodeId,
    CurrentAttributeValues::ValueBool,
    CurrentAttributeValues::ValueNumber,
    CurrentAttributeValues::ValueString,
    CurrentAttributeValues::ValueGeo,
    CurrentAttributeValues::ValueObject,
    CurrentAttributeValues::ValueUri,
    CurrentAttributeValues::ValueLongText,
];

/// All persisted projection columns, in table order.
pub(super) const PROJECTION_COLUMNS: [CurrentAttributeValues; 14] = [
    CurrentAttributeValues::EntityKey,
    CurrentAttributeValues::AttributeNameId,
    CurrentAttributeValues::AttributeKind,
    CurrentAttributeValues::ValueKind,
    CurrentAttributeValues::ValueDatetime,
    CurrentAttributeValues::UnitCodeId,
    CurrentAttributeValues::ValueBool,
    CurrentAttributeValues::DatasetId,
    CurrentAttributeValues::ValueNumber,
    CurrentAttributeValues::ValueString,
    CurrentAttributeValues::ValueGeo,
    CurrentAttributeValues::ValueObject,
    CurrentAttributeValues::ValueUri,
    CurrentAttributeValues::ValueLongText,
];

/// The fourteen staging columns, in the order the batch binds them.
///
/// Each pairs the name the row column takes in the `AS d(…)` alias with the
/// array element type its `UNNEST` argument casts to. The `value_number` array
/// is `numeric[]`, so exact decimals are bound without a text or binary64 hop.
pub(super) fn staging_columns() -> [UnnestColumn; 14] {
    [
        UnnestColumn::new(CurrentAttributeValues::EntityKey, PostgresType::BigintArray),
        UnnestColumn::new(CurrentAttributeValues::AttributeNameId, PostgresType::IntegerArray),
        UnnestColumn::new(CurrentAttributeValues::AttributeKind, PostgresType::SmallintArray),
        UnnestColumn::new(CurrentAttributeValues::ValueKind, PostgresType::SmallintArray),
        UnnestColumn::new(CurrentAttributeValues::ValueDatetime, PostgresType::TimestamptzArray),
        UnnestColumn::new(CurrentAttributeValues::UnitCodeId, PostgresType::SmallintArray),
        UnnestColumn::new(CurrentAttributeValues::ValueBool, PostgresType::BooleanArray),
        UnnestColumn::new(CurrentAttributeValues::DatasetId, PostgresType::TextArray),
        UnnestColumn::new(CurrentAttributeValues::ValueNumber, PostgresType::NumericArray),
        UnnestColumn::new(CurrentAttributeValues::ValueString, PostgresType::TextArray),
        UnnestColumn::new(CurrentAttributeValues::ValueGeo, PostgresType::ByteaArray),
        UnnestColumn::new(CurrentAttributeValues::ValueObject, PostgresType::JsonbArray),
        UnnestColumn::new(CurrentAttributeValues::ValueUri, PostgresType::TextArray),
        UnnestColumn::new(CurrentAttributeValues::ValueLongText, PostgresType::TextArray),
    ]
}
