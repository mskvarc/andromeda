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
//! Refining the shape therefore has to happen where the row is read, from the
//! value itself. Both splits are decidable because the write path stores a
//! canonical rendering: `NgsiLdTime` reaches the text column as
//! `HH:MM:SS.ffffffZ` and `NgsiLdDate` as `YYYY-MM-DD`, and both forms are
//! fixed-width, so a pattern that pins the whole value recognises exactly what
//! that path wrote.

use crate::alias::Ah;
use andromeda_postgresql_codes::{compact_code_domain::CompactCodeDomain, registry::UnknownCompactCode, value_kind::ValueKind};
use andromeda_postgresql_schema::temporal::AttributeHistory;
use andromeda_postgresql_seaquery::{
    cast::cast,
    jsonb::jsonb_typeof,
    literal::{int_literal, text_literal},
    operator::regex_match,
    pg_type::PostgresType,
};
use sea_query::{CaseStatement, Expr, ExprTrait, Iden, SimpleExpr};
use strum::Display;

/// The name this shape is projected under by [`recorded_shape`].
#[derive(Iden, Clone, Copy)]
#[iden = "recorded_shape"]
pub struct RecordedShapeColumn;

/// The whole-value pattern the canonical `NgsiLdTime` rendering matches.
///
/// Anchored at both ends and fixed-width: the write path canonicalises every
/// `Time` to UTC with exactly six fractional digits, so a text value that is
/// not that shape was never written as a `Time`.
pub(crate) const CANONICAL_TIME_PATTERN: &str = "^[0-9]{2}:[0-9]{2}:[0-9]{2}[.][0-9]{6}Z$";

/// The whole-value pattern the canonical `NgsiLdDate` rendering matches.
const CANONICAL_DATE_PATTERN: &str = "^[0-9]{4}-[0-9]{2}-[0-9]{2}$";

/// The JSON type name `jsonb_typeof` reports for an array.
const JSON_ARRAY_TYPE: &str = "array";

/// The value shape one history row was recorded in.
///
/// One member per column of Tables 4.5.19.1-1 and 4.5.19.1-2, plus the URI a
/// Relationship object is held as, which Table 4.5.19.1-3 aggregates. The
/// numbering is a projection detail rather than a stored contract — no row
/// carries it — so it is dense and unrelated to [`ValueKind`]'s; only
/// equality of two codes is read, never their order.
///
/// Andromeda owns this vocabulary rather than the specification, so the labels
/// are kebab-case.
#[derive(Clone, Copy, Debug, Display, Eq, Hash, PartialEq)]
#[strum(serialize_all = "kebab-case")]
pub enum RecordedShape {
    /// A JSON number.
    Number,
    /// A JSON string that is neither of the two canonical temporal renderings.
    String,
    /// A JSON boolean.
    Boolean,
    /// A `GeoJSON` geometry.
    Geometry,
    /// A JSON object.
    Object,
    /// A JSON array, whose aggregates are taken over its size.
    Array,
    /// A clause 4.6.3 `DateTime`.
    DateTime,
    /// A clause 4.6.3 `Date`.
    Date,
    /// A clause 4.6.3 `Time`.
    Time,
    /// A URI, which is how a Relationship object is held.
    Uri,
}

impl RecordedShape {
    /// Every shape, in projection-code order.
    pub const ALL: [RecordedShape; 10] = [
        RecordedShape::Number,
        RecordedShape::String,
        RecordedShape::Boolean,
        RecordedShape::Geometry,
        RecordedShape::Object,
        RecordedShape::Array,
        RecordedShape::DateTime,
        RecordedShape::Date,
        RecordedShape::Time,
        RecordedShape::Uri,
    ];

    /// The number this shape is projected as.
    #[must_use]
    pub const fn code(self) -> i16 {
        match self {
            RecordedShape::Number => 1,
            RecordedShape::String => 2,
            RecordedShape::Boolean => 3,
            RecordedShape::Geometry => 4,
            RecordedShape::Object => 5,
            RecordedShape::Array => 6,
            RecordedShape::DateTime => 7,
            RecordedShape::Date => 8,
            RecordedShape::Time => 9,
            RecordedShape::Uri => 10,
        }
    }
}

// Manual rather than derived: `derive_more::TryFrom` yields its own repr error,
// and the aggregation decoder reads every projected code through one
// `UnknownCompactCode`-shaped conversion so that a shape and a registry code
// fail to decode the same way. No derive can name a foreign error type.
impl TryFrom<i16> for RecordedShape {
    type Error = UnknownCompactCode;

    fn try_from(code: i16) -> Result<RecordedShape, UnknownCompactCode> {
        match code {
            1 => Ok(RecordedShape::Number),
            2 => Ok(RecordedShape::String),
            3 => Ok(RecordedShape::Boolean),
            4 => Ok(RecordedShape::Geometry),
            5 => Ok(RecordedShape::Object),
            6 => Ok(RecordedShape::Array),
            7 => Ok(RecordedShape::DateTime),
            8 => Ok(RecordedShape::Date),
            9 => Ok(RecordedShape::Time),
            10 => Ok(RecordedShape::Uri),
            other => Err(UnknownCompactCode::new(CompactCodeDomain::RecordedShape, other)),
        }
    }
}

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
    let value_object = || Expr::col((Ah, AttributeHistory::ValueObject));
    let value_string = || Expr::col((Ah, AttributeHistory::ValueString));
    // An integer literal is `int4` in PostgreSQL, but the code the decoder reads
    // is the same `smallint` the `value_kind` column it refines is stored as, so
    // each branch is cast rather than leaving the CASE a wider type than the row
    // it describes.
    let shape = |shape: RecordedShape| cast(int_literal(i64::from(shape.code())), PostgresType::Smallint);

    CaseStatement::new()
        .case(
            value_kind_is(ValueKind::Object).and(Expr::expr(jsonb_typeof(value_object())).eq(JSON_ARRAY_TYPE)),
            shape(RecordedShape::Array),
        )
        .case(value_kind_is(ValueKind::Object), shape(RecordedShape::Object))
        .case(
            value_kind_is(ValueKind::String).and(regex_match(value_string(), text_literal(CANONICAL_TIME_PATTERN))),
            shape(RecordedShape::Time),
        )
        .case(
            value_kind_is(ValueKind::String).and(regex_match(value_string(), text_literal(CANONICAL_DATE_PATTERN))),
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
    use crate::recorded_shape::{CANONICAL_DATE_PATTERN, CANONICAL_TIME_PATTERN, RecordedShape, recorded_shape};
    use andromeda_postgresql_codes::{compact_code_domain::CompactCodeDomain, registry::UnknownCompactCode};
    use sea_query::{PostgresQueryBuilder, Query};

    fn rendered() -> String {
        Query::select().expr(recorded_shape()).to_owned().to_string(PostgresQueryBuilder)
    }

    /// Every shape decodes back to itself, so a projected code and the member
    /// it stands for cannot drift apart.
    #[test]
    fn every_shape_round_trips_through_its_projection_code() {
        for shape in RecordedShape::ALL {
            pretty_assertions::assert_eq!(RecordedShape::try_from(shape.code()), Ok(shape));
        }
    }

    /// No two members share a code; a shared code would silently merge two
    /// columns of the aggregation tables back together.
    #[test]
    fn no_two_shapes_share_a_projection_code() {
        for shape in RecordedShape::ALL {
            let sharing = RecordedShape::ALL.iter().filter(|other| other.code() == shape.code()).count();
            pretty_assertions::assert_eq!(sharing, 1, "{shape} shares its code");
        }
    }

    /// A number the projection never emits fails to decode rather than
    /// resolving to a default shape.
    #[test]
    fn a_code_no_shape_carries_fails_to_decode() {
        for code in [0, 11, -1] {
            pretty_assertions::assert_eq!(
                RecordedShape::try_from(code),
                Err(UnknownCompactCode::new(CompactCodeDomain::RecordedShape, code)),
                "{code}"
            );
        }
    }

    /// Table 4.5.19.1-1 gives a JSON Array `avg`/`min`/`max`/`sum` over its
    /// sizes and a JSON Object none of them, so the two have to be separated
    /// inside the one JSONB column that holds both.
    #[test]
    fn an_array_is_separated_from_an_object_by_its_json_type() {
        let sql = rendered();
        assert!(sql.contains("jsonb_typeof(\"ah\".\"value_object\")"), "{sql}");
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
        assert!(sql.contains(CANONICAL_TIME_PATTERN), "{sql}");
        assert!(sql.contains(CANONICAL_DATE_PATTERN), "{sql}");
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
        for pattern in [CANONICAL_TIME_PATTERN, CANONICAL_DATE_PATTERN] {
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
