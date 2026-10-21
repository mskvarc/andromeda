//! The value shape one `attribute_history` row was recorded in, at the grain
//! Tables 4.5.19.1-1, 4.5.19.1-2 and 4.5.19.1-3 distinguish.
//!
//! The stored `value_kind` code names the typed column a row's value sits in,
//! which is coarser than the tables are. One JSONB column holds both a JSON
//! Object and a JSON Array, and the tables give the two different methods:
//! `avg`, `min`, `max` and `sum` are defined over the sizes of arrays and are
//! N/A over objects. One text column holds a JSON String, a clause 4.6.3 `Date`
//! and a clause 4.6.3 `Time`, and the tables give `Time` an `avg` the other two
//! do not have.
//!
//! Refining the shape therefore has to happen from the value itself: where a
//! row is recorded, for the shapes the stream catalogue keeps per stream, and
//! where it is aggregated. Both splits are decidable because the write path
//! stores a
//! canonical rendering: `NgsiLdTime` reaches the text column as
//! `HH:MM:SS.ffffffZ` and `NgsiLdDate` as `YYYY-MM-DD`, and both forms are
//! fixed-width, so a pattern that pins the whole value recognises exactly what
//! that path wrote.

use crate::{alias::Ah, structured_json::structured_json};
use andromeda_ngsi_ld_lexicon::time::{date::NgsiLdDate, time_of_day::NgsiLdTime};
use andromeda_postgresql_codes::{recorded_shape::RecordedShape, value_kind::ValueKind};
use andromeda_postgresql_schema::temporal::AttributeHistory;
use andromeda_postgresql_seaquery::{
    cast::cast,
    jsonb::jsonb_typeof,
    literal::{int_literal, text_literal},
    operator::regex_match,
    pg_type::PostgresType,
};
use sea_query::{CaseStatement, Expr, ExprTrait, Iden, SimpleExpr};

/// The name this shape is projected under by [`recorded_shape`].
#[derive(Iden, Clone, Copy)]
#[iden = "recorded_shape"]
pub struct RecordedShapeColumn;

/// The JSON type name `jsonb_typeof` reports for an array.
const JSON_ARRAY_TYPE: &str = "array";

/// Whether the row's stored value shape is `kind`.
fn value_kind_is(kind: ValueKind) -> SimpleExpr {
    Expr::col((Ah, AttributeHistory::ValueKind)).eq(int_literal(i64::from(kind.code())))
}

/// The refined shape of the `attribute_history` row aliased `ah`.
///
/// The two refinements are tested before the shapes they refine, so a JSONB
/// array never reaches the object branch and a canonical temporal rendering
/// never reaches the plain-string branch.
///
/// A tombstone row yields SQL NULL: clause 4.5.7 gives a deletion no value at
/// all, so it has no shape, and every statement that projects this excludes
/// tombstone rows rather than asking what shape one has.
#[must_use]
pub fn recorded_shape() -> SimpleExpr {
    let value_string = || Expr::col((Ah, AttributeHistory::ValueString));
    // An integer literal is `int4` in PostgreSQL, but the code the decoder reads
    // is the same `smallint` the `value_kind` column it refines is stored as, so
    // each branch is cast rather than leaving the CASE a wider type than the row
    // it describes.
    let shape = |shape: RecordedShape| cast(int_literal(i64::from(shape.code())), PostgresType::Smallint);

    CaseStatement::new()
        .case(
            value_kind_is(ValueKind::Object).and(Expr::expr(jsonb_typeof(structured_json())).eq(JSON_ARRAY_TYPE)),
            shape(RecordedShape::Array),
        )
        .case(value_kind_is(ValueKind::Object), shape(RecordedShape::Object))
        .case(
            value_kind_is(ValueKind::String).and(regex_match(value_string(), text_literal(NgsiLdTime::STORED_PATTERN))),
            shape(RecordedShape::Time),
        )
        .case(
            value_kind_is(ValueKind::String).and(regex_match(value_string(), text_literal(NgsiLdDate::STORED_PATTERN))),
            shape(RecordedShape::Date),
        )
        .case(value_kind_is(ValueKind::String), shape(RecordedShape::String))
        .case(value_kind_is(ValueKind::Number), shape(RecordedShape::Number))
        .case(value_kind_is(ValueKind::Boolean), shape(RecordedShape::Boolean))
        .case(value_kind_is(ValueKind::Geometry), shape(RecordedShape::Geometry))
        .case(value_kind_is(ValueKind::DateTime), shape(RecordedShape::DateTime))
        .case(value_kind_is(ValueKind::Uri), shape(RecordedShape::Uri))
        .into()
}

#[cfg(test)]
mod tests {
    use crate::recorded_shape::recorded_shape;
    use andromeda_ngsi_ld_lexicon::time::{date::NgsiLdDate, time_of_day::NgsiLdTime};
    use sea_query::{PostgresQueryBuilder, Query};

    fn rendered() -> String {
        Query::select().expr(recorded_shape()).to_owned().to_string(PostgresQueryBuilder)
    }

    /// Table 4.5.19.1-1 gives a JSON Array `avg`/`min`/`max`/`sum` over its
    /// sizes and a JSON Object none of them, so the two have to be separated
    /// inside the one JSONB column that holds both.
    #[test]
    fn an_array_is_separated_from_an_object_by_its_json_type() {
        let sql = rendered();
        assert!(
            sql.contains("jsonb_typeof(COALESCE((CASE WHEN ((\"ah\".\"value_object\" ->> ('@type')) = ('@json'))"),
            "{sql}"
        );
        assert!(sql.contains("THEN 6"), "the array branch must be reachable: {sql}");
        assert!(sql.contains("THEN 5"), "the object branch must be reachable: {sql}");
        let array_branch = sql.find("THEN 6").expect("an array branch");
        let object_branch = sql.find("THEN 5").expect("an object branch");
        assert!(array_branch < object_branch, "the array test must precede the object fallback: {sql}");
    }

    /// Table 4.5.19.1-2 gives `Time` an `avg` that the `Date` and the JSON
    /// String sharing its text column do not have, so all three are separated
    /// by the canonical rendering the write path stores.
    #[test]
    fn a_time_and_a_date_are_separated_from_a_plain_string_by_their_canonical_form() {
        let sql = rendered();
        assert!(sql.contains(NgsiLdTime::STORED_PATTERN), "{sql}");
        assert!(sql.contains(NgsiLdDate::STORED_PATTERN), "{sql}");
        let time_branch = sql.find("THEN 9").expect("a time branch");
        let date_branch = sql.find("THEN 8").expect("a date branch");
        let string_branch = sql.find("THEN 2").expect("a string branch");
        assert!(time_branch < string_branch && date_branch < string_branch, "{sql}");
    }

    /// The patterns are anchored at both ends: an unanchored `Time` pattern
    /// would classify any string that merely contains a time of day, and the
    /// aggregation tables would then offer `avg` over it.
    #[test]
    fn both_temporal_patterns_pin_the_whole_value() {
        for pattern in [NgsiLdTime::STORED_PATTERN, NgsiLdDate::STORED_PATTERN] {
            assert!(pattern.starts_with('^'), "{pattern}");
            assert!(pattern.ends_with('$'), "{pattern}");
        }
    }

    /// Clause 4.5.7: a deletion carries no value, so it has no shape either -
    /// the projection has no fallback branch that would give it one.
    #[test]
    fn a_tombstone_row_falls_out_of_the_projection_with_no_shape() {
        let sql = rendered();
        assert!(!sql.contains("ELSE"), "{sql}");
    }

    /// The patterns and the branch codes are rendered inline, so projecting a
    /// shape adds no placeholder and shifts no binding a caller made.
    #[test]
    fn the_projection_binds_no_parameter_of_its_own() {
        let (_sql, values) = Query::select().expr(recorded_shape()).to_owned().build(PostgresQueryBuilder);
        pretty_assertions::assert_eq!(values.0.len(), 1, "only the jsonb type name binds");
    }
}
