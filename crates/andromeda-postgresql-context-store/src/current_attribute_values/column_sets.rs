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
/// is a difference on two fields at once, and `expires_at` is among them, so an
/// instance whose value is unchanged but whose clause 4.22 `expiresAt` moved is
/// rewritten.
pub(crate) const REFRESHED_COLUMNS: [CurrentAttributeValues; 12] = [
    CurrentAttributeValues::AttributeKind,
    CurrentAttributeValues::ValueKind,
    CurrentAttributeValues::ValueDateTime,
    CurrentAttributeValues::ExpiresAt,
    CurrentAttributeValues::UnitCodeId,
    CurrentAttributeValues::ValueBoolean,
    CurrentAttributeValues::ValueNumber,
    CurrentAttributeValues::ValueString,
    CurrentAttributeValues::ValueGeometry,
    CurrentAttributeValues::ValueObject,
    CurrentAttributeValues::ValueUri,
    CurrentAttributeValues::ValueLongText,
];

/// All persisted projection columns, in table order.
pub(crate) const PROJECTION_COLUMNS: [CurrentAttributeValues; 15] = [
    CurrentAttributeValues::EntityKey,
    CurrentAttributeValues::AttributeNameKey,
    CurrentAttributeValues::AttributeKind,
    CurrentAttributeValues::ValueKind,
    CurrentAttributeValues::ValueDateTime,
    CurrentAttributeValues::ExpiresAt,
    CurrentAttributeValues::UnitCodeId,
    CurrentAttributeValues::ValueBoolean,
    CurrentAttributeValues::DatasetId,
    CurrentAttributeValues::ValueNumber,
    CurrentAttributeValues::ValueString,
    CurrentAttributeValues::ValueGeometry,
    CurrentAttributeValues::ValueObject,
    CurrentAttributeValues::ValueUri,
    CurrentAttributeValues::ValueLongText,
];

/// The fifteen staging columns, in the order the batch binds them.
///
/// Each pairs the name the row column takes in the `AS d(…)` alias with the
/// array element type its `UNNEST` argument casts to. The `value_number` array
/// is `numeric[]`, so exact decimals are bound without a text or binary64 hop.
pub(crate) fn staging_columns() -> [UnnestColumn; 15] {
    [
        UnnestColumn::new(CurrentAttributeValues::EntityKey, PostgresType::BigintArray),
        UnnestColumn::new(CurrentAttributeValues::AttributeNameKey, PostgresType::IntegerArray),
        UnnestColumn::new(CurrentAttributeValues::AttributeKind, PostgresType::SmallintArray),
        UnnestColumn::new(CurrentAttributeValues::ValueKind, PostgresType::SmallintArray),
        UnnestColumn::new(CurrentAttributeValues::ValueDateTime, PostgresType::TimestamptzArray),
        UnnestColumn::new(CurrentAttributeValues::ExpiresAt, PostgresType::TimestamptzArray),
        UnnestColumn::new(CurrentAttributeValues::UnitCodeId, PostgresType::SmallintArray),
        UnnestColumn::new(CurrentAttributeValues::ValueBoolean, PostgresType::BooleanArray),
        UnnestColumn::new(CurrentAttributeValues::DatasetId, PostgresType::TextArray),
        UnnestColumn::new(CurrentAttributeValues::ValueNumber, PostgresType::NumericArray),
        UnnestColumn::new(CurrentAttributeValues::ValueString, PostgresType::TextArray),
        UnnestColumn::new(CurrentAttributeValues::ValueGeometry, PostgresType::ByteaArray),
        UnnestColumn::new(CurrentAttributeValues::ValueObject, PostgresType::JsonbArray),
        UnnestColumn::new(CurrentAttributeValues::ValueUri, PostgresType::TextArray),
        UnnestColumn::new(CurrentAttributeValues::ValueLongText, PostgresType::TextArray),
    ]
}
