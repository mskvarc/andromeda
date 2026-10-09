//! Which typed field of a stored instance holds a value of each clause
//! 4.9 operand kind, and how a Query Term is compared against it.
//!
//! The mapping mirrors what the write path stores:
//!
//! * numbers land in `value_number` (`numeric`) and bind directly as an exact
//!   decimal, so every supported operand uses the typed numeric index,
//! * booleans in `value_boolean`,
//! * date-times in `value_date_time`,
//! * strings in `value_string` for Properties and single-term
//!   `VocabProperties` (or, past the B-tree bound, in `value_long_text`, which
//!   [`ValueColumn::of_string`] decides from the byte length alone) and,
//!   under the equality operators only, in `value_uri` for Relationship objects
//!   (or, past the B-tree bound, in `value_long_uri`, which
//!   [`ValueColumn::of_uri`] decides from the byte length alone), which is what
//!   [`relationship_object_is_comparable`] decides,
//! * plain dates and times in `value_string`, because only full
//!   `DateTime`-typed values are stored in `value_date_time`. Both are stored
//!   in the one canonical, fixed-width form their clause 4.6.3 types define, so
//!   byte-wise order over the column is chronological order and an ordering
//!   comparison against the bound canonical text is a temporal comparison. Their
//!   canonical text is always short, so they never read `value_long_text`.
//!
//! Every value whose family is not a single scalar lives in `value_object`
//! instead; [`crate::q_predicate::structured_target`] compares those.

use crate::{
    operator::compare,
    q_predicate::{instance_row::InstanceRow, string_ordering::ordered_string, value_column::ValueColumn},
};
use andromeda_ngsi_ld_lexicon::{ngsi_ld_date::NgsiLdDate, ngsi_ld_time::NgsiLdTime};
use andromeda_ngsi_ld_query::filter::{predicate::CmpOp, scalar::Scalar};
use andromeda_postgresql_seaquery::literal::false_literal;
use sea_query::{Expr as SeaExpr, ExprTrait, SimpleExpr};

/// Compare one scalar against the typed column that holds values of its kind.
///
/// Every arm names exactly one typed column and compares it to a bound value, so
/// each comparison is the shape the partial index over that column is predicated
/// on: `value_number > $1` implies `value_number IS NOT NULL`, which is what
/// makes `(attribute_name_key, value_number) WHERE value_number IS NOT NULL`
/// applicable. Nothing tests `value_kind`, deliberately: the row's check
/// constraint already ties the kind to the populated column, so the column being
/// non-NULL *is* the kind test.
///
/// A row whose value lives in another column therefore yields NULL, not FALSE,
/// which is the reading clause 4.9 wants under every operator compiled here.
/// `equal` and the ordering operators all state it the same way: "If there is
/// no equality between the target value data type and the Query Term value data
/// type, then it shall be considered as not matching", and an unknown that the
/// enclosing `EXISTS` drops is exactly that.
///
/// `unequal` is the one operator that reads a datatype difference the other way,
/// and it is not compiled here: clause 4.9 makes such a target *unequal*, so
/// `crate::q_predicate::projection` compiles it as the total negation of the
/// whole `equal` condition rather than as a per-column `!=` that a NULL would
/// swallow.
#[must_use]
pub fn typed_column_condition(scalar: &Scalar, op: CmpOp, row: InstanceRow) -> SimpleExpr {
    match scalar {
        // SeaQuery owns every value in its AST, so this is the one necessary
        // clone between the borrowed query domain value and the NUMERIC bind.
        Scalar::Number(number) => compare(row.value(ValueColumn::Number), op, SeaExpr::val(number.decimal().clone())),
        Scalar::Bool(value) => compare(row.value(ValueColumn::Boolean), op, SeaExpr::val(*value)),
        Scalar::Str(value) | Scalar::Uri(value) => {
            let string_test = string_condition(scalar, value.as_str(), op, row);
            match relationship_object_is_comparable(op) {
                RelationshipObjects::Comparable => {
                    // Equality is byte identity, so a stored object equal to the
                    // operand sits in the one column the operand would be placed
                    // in, and only that column is read.
                    let uri_test = compare(row.value(ValueColumn::of_uri(value.as_str())), op, SeaExpr::val(value.as_str()));
                    string_test.or(uri_test)
                }
                RelationshipObjects::NonMatching => string_test,
            }
        }
        Scalar::DateTime(value) => compare(row.value(ValueColumn::DateTime), op, SeaExpr::val(*value)),
        Scalar::Date(value) => compare(row.value(ValueColumn::String), op, SeaExpr::val(NgsiLdDate::from(*value).to_string())),
        // The operand is rendered in the same canonical form the write path
        // stores, rather than in the clause 4.9 serialisation the client wrote:
        // `12:00:00+01:00` and `11:00:00Z` are one time of day, and only the
        // canonical text of both compares against the stored column.
        Scalar::Time(value) => match NgsiLdTime::normalise(*value) {
            Ok(canonical) => compare(row.value(ValueColumn::String), op, SeaExpr::val(canonical.to_string())),
            // A time of day with no canonical form is one no write path could
            // have stored, so nothing can compare equal to it.
            Err(_) => false_literal(),
        },
    }
}

/// Compare a String operand against the two string columns (clause 4.9).
///
/// Equality is byte identity under `COLLATE "C"`: a stored value equal to the
/// operand has the operand's length, so it sits in the one column the operand
/// itself would be placed in, and only that column is read. A short `==`
/// therefore compiles exactly as it would with no overflow column at all. An
/// ordering reads both columns, because a stored value of any length may order
/// on either side of the operand; a row whose value is in the other column
/// yields NULL for that arm, which the disjunction absorbs. An ordering
/// compares under the collation [`ordered_string`] picks.
fn string_condition(scalar: &Scalar, value: &str, op: CmpOp, row: InstanceRow) -> SimpleExpr {
    match op {
        CmpOp::Equal | CmpOp::Unequal => compare(row.value(ValueColumn::of_string(value)), op, SeaExpr::val(value)),
        CmpOp::Greater | CmpOp::GreaterEq | CmpOp::Less | CmpOp::LessEq => {
            let ordered = |column: &ValueColumn| compare(ordered_string(row.value(*column), scalar, op), op, SeaExpr::val(value));
            row.string_columns().iter().map(ordered).reduce(ExprTrait::or).unwrap_or_else(false_literal)
        }
    }
}

/// Whether a Relationship's object is a comparison target under one operator.
///
/// `value_uri` and `value_long_uri` hold Relationship
/// objects and nothing else, so probing them is exactly the decision of whether
/// the predicate may match a Relationship.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RelationshipObjects {
    /// The operator admits a Relationship's object as a comparison target.
    Comparable,
    /// Clause 4.9 requires a Relationship operand to non-match this operator.
    NonMatching,
}

/// Clause 4.9: a Relationship (and a `ListRelationship`) operand shall non-match
/// every operator other than equality and inequality.
///
/// A URI names a resource; it carries no order a `<` or `>` could mean, and the
/// pattern operators are defined over String targets. Probing `value_uri` under
/// an ordering operator would make `owner>"a"` select entities the standard
/// requires it to reject.
///
/// The objects of a `ListRelationship`, or of a Relationship holding several,
/// are not here at all: they are held in `value_object` as a JSON array, and
/// the containment test in
/// [`crate::q_predicate::structured_target`] is what compares a URI Query Term
/// against them.
const fn relationship_object_is_comparable(op: CmpOp) -> RelationshipObjects {
    match op {
        CmpOp::Equal | CmpOp::Unequal => RelationshipObjects::Comparable,
        CmpOp::Greater | CmpOp::GreaterEq | CmpOp::Less | CmpOp::LessEq => RelationshipObjects::NonMatching,
    }
}

#[cfg(test)]
mod tests {
    use crate::q_predicate::{
        instance_row::{ElementLevel, InstanceRow},
        typed_column::typed_column_condition,
    };
    use andromeda_ngsi_ld_query::filter::{
        predicate::CmpOp,
        scalar::{Scalar, parse_date, parse_time},
    };
    use andromeda_postgresql_schema::{string_value_column::INDEXED_STRING_BYTE_LIMIT, uri_value_column::INDEXED_URI_BYTE_LIMIT};
    use sea_query::{PostgresQueryBuilder, Query, SimpleExpr};

    /// The comparison over a current row.
    fn current(scalar: &Scalar, op: CmpOp) -> SimpleExpr {
        typed_column_condition(scalar, op, InstanceRow::Current)
    }

    fn render(expr: SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    #[test]
    fn each_operand_kind_names_the_column_the_write_path_projects_it_into() {
        for (scalar, column) in [
            (Scalar::Number(1.into()), "value_number"),
            (Scalar::Bool(true), "value_boolean"),
            (Scalar::Str("red".to_owned()), "value_string"),
        ] {
            let sql = render(current(&scalar, CmpOp::Equal));
            assert!(sql.contains(column), "{scalar:?}: {sql}");
        }
    }

    /// Clause 4.9: a Relationship operand shall non-match every operator other
    /// than equality and inequality, and `value_uri` holds Relationship objects
    /// and nothing else.
    #[test]
    fn only_the_equality_operators_reach_relationship_objects() {
        let scalar = Scalar::Str("urn:ngsi-ld:Person:1".to_owned());
        for op in [CmpOp::Equal, CmpOp::Unequal] {
            assert!(render(current(&scalar, op)).contains("value_uri"), "{op:?}");
        }
        for op in [CmpOp::Greater, CmpOp::GreaterEq, CmpOp::Less, CmpOp::LessEq] {
            assert!(!render(current(&scalar, op)).contains("value_uri"), "{op:?}");
        }
    }

    /// A Relationship object past the B-tree bound is projected into
    /// `value_long_uri`, so an equality on an operand that long reads that
    /// column and never `value_uri`, where no such object can be.
    #[test]
    fn a_uri_equality_past_the_limit_reads_only_the_overflow_uri_column() {
        let long = Scalar::Str(format!("urn:ngsi-ld:Person:{}", "a".repeat(6000)));
        for op in [CmpOp::Equal, CmpOp::Unequal] {
            let sql = render(current(&long, op));
            assert!(sql.contains(r#""ca"."value_long_uri" "#), "{op:?}: {sql}");
            assert!(!sql.contains("value_uri"), "{op:?}: {sql}");
        }
    }

    #[test]
    fn a_uri_equality_at_the_limit_reads_only_the_indexed_uri_column() {
        let prefix = "urn:ngsi-ld:Person:";
        let at_limit = format!("{prefix}{}", "a".repeat(INDEXED_URI_BYTE_LIMIT - prefix.len()));
        let sql = render(current(&Scalar::Str(at_limit), CmpOp::Equal));
        assert!(sql.contains(r#""ca"."value_uri" = "#), "{sql}");
        assert!(!sql.contains("value_long_uri"), "{sql}");
    }

    /// History keeps a URI of any length in `value_uri`, so a long URI
    /// equality over history reads that column.
    #[test]
    fn a_long_uri_equality_over_history_reads_the_one_history_uri_column() {
        let long = format!("urn:ngsi-ld:Person:{}", "a".repeat(6000));
        let sql = render(typed_column_condition(&Scalar::Str(long), CmpOp::Equal, InstanceRow::History));
        assert!(sql.contains(r#""qh"."value_uri" = "#), "{sql}");
        assert!(!sql.contains("value_long_uri"), "{sql}");
    }

    /// An element keeps a String of any length in `value_string`, so its
    /// ordering reads that field alone.
    #[test]
    fn an_element_string_ordering_reads_its_one_string_field() {
        let sql = render(typed_column_condition(
            &Scalar::Str("m".to_owned()),
            CmpOp::Greater,
            InstanceRow::Element(ElementLevel::at(0)),
        ));
        assert!(sql.contains(r#"("l0"."value_string") COLLATE "und-x-icu""#), "{sql}");
        assert!(!sql.contains("value_long_text"), "{sql}");
    }

    #[test]
    fn a_short_string_equality_reads_only_the_indexed_column() {
        let sql = render(current(&Scalar::Str("red".to_owned()), CmpOp::Equal));
        assert!(sql.contains(r#""ca"."value_string" = "#), "{sql}");
        assert!(!sql.contains("value_long_text"), "{sql}");
    }

    #[test]
    fn a_string_equality_past_the_limit_reads_only_the_overflow_column() {
        let long = "a".repeat(INDEXED_STRING_BYTE_LIMIT + 1);
        let sql = render(current(&Scalar::Str(long), CmpOp::Equal));
        assert!(sql.contains(r#""ca"."value_long_text" = "#), "{sql}");
        assert!(!sql.contains("value_string"), "{sql}");
    }

    #[test]
    fn string_ordering_reads_both_string_columns_and_never_relationship_objects() {
        for op in [CmpOp::Greater, CmpOp::GreaterEq, CmpOp::Less, CmpOp::LessEq] {
            let sql = render(current(&Scalar::Str("m".to_owned()), op));
            assert!(sql.contains(r#"("ca"."value_string") COLLATE "und-x-icu""#), "{op:?}: {sql}");
            assert!(sql.contains(r#"("ca"."value_long_text") COLLATE "und-x-icu""#), "{op:?}: {sql}");
            assert!(!sql.contains("value_uri"), "{op:?}: {sql}");
        }
    }

    /// RFC 8259 section 8.3 makes String equality code unit identity, so an
    /// equality keeps the byte order of the column and its index.
    #[test]
    fn a_string_equality_compares_under_the_column_byte_order() {
        for op in [CmpOp::Equal, CmpOp::Unequal] {
            assert!(!render(current(&Scalar::Str("m".to_owned()), op)).contains("COLLATE"), "{op:?}");
        }
    }

    /// History keeps a String of any length in `value_string`, so a long
    /// String equality over history reads that column.
    #[test]
    fn a_long_string_equality_over_history_reads_the_one_history_string_column() {
        let long = "a".repeat(INDEXED_STRING_BYTE_LIMIT + 1);
        let sql = render(typed_column_condition(&Scalar::Str(long), CmpOp::Equal, InstanceRow::History));
        assert!(sql.contains(r#""qh"."value_string" = "#), "{sql}");
        assert!(!sql.contains("value_long_text"), "{sql}");
    }

    #[test]
    fn date_and_time_operands_never_read_the_overflow_column() {
        let date = parse_date("2024-01-15").expect("a date");
        let time = parse_time("12:00:00Z").expect("a time");
        for scalar in [Scalar::Date(date), Scalar::Time(time)] {
            for op in [CmpOp::Equal, CmpOp::Greater, CmpOp::Less] {
                let sql = render(current(&scalar, op));
                assert!(sql.contains("value_string"), "{scalar:?} {op:?}: {sql}");
                assert!(!sql.contains("value_long_text"), "{scalar:?} {op:?}: {sql}");
            }
        }
    }

    /// Clause 4.6.3 admits a `Time` only with the literal `Z` designator, and
    /// the write path stores one canonical rendering, so an operand that cannot
    /// be normalised is one nothing stored could equal.
    #[test]
    fn a_time_operand_that_normalises_compares_against_its_canonical_text() {
        let time = parse_time("12:00:00,5Z").expect("the comma compatibility form");
        let sql = render(current(&Scalar::Time(time), CmpOp::Equal));
        assert!(sql.contains("value_string"), "{sql}");
    }
}
