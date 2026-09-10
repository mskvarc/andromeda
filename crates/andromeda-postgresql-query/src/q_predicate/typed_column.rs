//! Which typed `current_attribute_values` column holds a value of each clause
//! 4.9 operand kind, and how a Query Term is compared against it.
//!
//! The mapping mirrors what the write path projects:
//!
//! * numbers land in `value_number` (`numeric`) and bind directly as an exact
//!   decimal, so every supported operand uses the typed numeric index,
//! * booleans in `value_bool`,
//! * date-times in `value_datetime`,
//! * strings in `value_string` for Properties and single-term
//!   `VocabProperties`, and — under the equality operators only — in `value_uri`
//!   for Relationship objects, which is what [`relationship_object_is_comparable`]
//!   decides,
//! * plain dates and times in `value_string`, because only full
//!   `DateTime`-typed values are projected to `value_datetime`. Both are stored
//!   in the one canonical, fixed-width form their clause 4.6.3 types define, so
//!   byte-wise order over the column is chronological order and an ordering
//!   comparison against the bound canonical text is a temporal comparison.
//!
//! Every value whose family is not a single scalar lives in `value_object`
//! instead; [`crate::q_predicate::structured_target`] compares those.

use crate::{operator::compare, q_predicate::projection_probe::probe_column};
use andromeda_ngsi_ld_lexicon::{ngsi_ld_date::NgsiLdDate, ngsi_ld_time::NgsiLdTime};
use andromeda_ngsi_ld_query::filter::{predicate::CmpOp, scalar::Scalar};
use andromeda_postgresql_schema::current::CurrentAttributeValues;
use andromeda_postgresql_seaquery::literal::false_literal;
use sea_query::{Expr as SeaExpr, ExprTrait, SimpleExpr};

/// Compare one scalar against the typed column that holds values of its kind.
///
/// Every arm names exactly one typed column and compares it to a bound value, so
/// each comparison is the shape the partial index over that column is predicated
/// on: `value_number > $1` implies `value_number IS NOT NULL`, which is what
/// makes `(attribute_name_id, value_number) WHERE value_number IS NOT NULL`
/// applicable. Nothing tests `value_kind`, deliberately: the projection's check
/// constraint already ties the kind to the populated column, so the column being
/// non-NULL *is* the kind test.
///
/// A row whose value lives in another column therefore yields NULL, not FALSE,
/// which is the reading clause 4.9 wants under every operator compiled here.
/// `equal` and the ordering operators all state it the same way — "If there is
/// no equality between the target value data type and the Query Term value data
/// type, then it shall be considered as not matching" — and an unknown that the
/// enclosing `EXISTS` drops is exactly that.
///
/// `unequal` is the one operator that reads a datatype difference the other way,
/// and it is not compiled here: clause 4.9 makes such a target *unequal*, so
/// `crate::q_predicate::projection` compiles it as the total negation of the
/// whole `equal` condition rather than as a per-column `!=` that a NULL would
/// swallow.
#[must_use]
pub fn typed_column_condition(scalar: &Scalar, op: CmpOp) -> SimpleExpr {
    match scalar {
        // SeaQuery owns every value in its AST, so this is the one necessary
        // clone between the borrowed query domain value and the NUMERIC bind.
        Scalar::Number(number) => compare(probe_column(CurrentAttributeValues::ValueNumber), op, SeaExpr::val(number.decimal().clone())),
        Scalar::Bool(value) => compare(probe_column(CurrentAttributeValues::ValueBool), op, SeaExpr::val(*value)),
        Scalar::Str(value) => {
            let string_test = compare(probe_column(CurrentAttributeValues::ValueString), op, SeaExpr::val(value.as_str()));
            match relationship_object_is_comparable(op) {
                RelationshipObjects::Comparable => {
                    let uri_test = compare(probe_column(CurrentAttributeValues::ValueUri), op, SeaExpr::val(value.as_str()));
                    string_test.or(uri_test)
                }
                RelationshipObjects::NonMatching => string_test,
            }
        }
        Scalar::DateTime(value) => compare(probe_column(CurrentAttributeValues::ValueDatetime), op, SeaExpr::val(*value)),
        Scalar::Date(value) => compare(
            probe_column(CurrentAttributeValues::ValueString),
            op,
            SeaExpr::val(NgsiLdDate::from(*value).to_string()),
        ),
        // The operand is rendered in the same canonical form the write path
        // stores, rather than in the clause 4.9 serialisation the client wrote:
        // `12:00:00+01:00` and `11:00:00Z` are one time of day, and only the
        // canonical text of both compares against the stored column.
        Scalar::Time(value) => match NgsiLdTime::normalise(*value) {
            Ok(canonical) => compare(probe_column(CurrentAttributeValues::ValueString), op, SeaExpr::val(canonical.to_string())),
            // A time of day with no canonical form is one no write path could
            // have stored, so nothing can compare equal to it.
            Err(_) => false_literal(),
        },
    }
}

/// Whether a Relationship's object is a comparison target under one operator.
///
/// `current_attribute_values.value_uri` holds Relationship objects and nothing
/// else, so probing it is exactly the decision of whether the predicate may
/// match a Relationship.
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
/// an ordering operator therefore made `owner>"a"` select entities the standard
/// requires it to reject.
///
/// A `ListRelationship`'s objects are not here at all: they are projected into
/// `value_object` as a JSON array, and the containment test in
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
    use crate::q_predicate::typed_column::typed_column_condition;
    use andromeda_ngsi_ld_query::filter::{
        predicate::CmpOp,
        scalar::{Scalar, parse_time},
    };
    use sea_query::{PostgresQueryBuilder, Query, SimpleExpr};

    fn render(expr: SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    #[test]
    fn each_operand_kind_names_the_column_the_write_path_projects_it_into() {
        for (scalar, column) in [
            (Scalar::Number(1.into()), "value_number"),
            (Scalar::Bool(true), "value_bool"),
            (Scalar::Str("red".to_owned()), "value_string"),
        ] {
            let sql = render(typed_column_condition(&scalar, CmpOp::Equal));
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
            assert!(render(typed_column_condition(&scalar, op)).contains("value_uri"), "{op:?}");
        }
        for op in [CmpOp::Greater, CmpOp::GreaterEq, CmpOp::Less, CmpOp::LessEq] {
            assert!(!render(typed_column_condition(&scalar, op)).contains("value_uri"), "{op:?}");
        }
    }

    /// Clause 4.6.3 admits a `Time` only with the literal `Z` designator, and
    /// the write path stores one canonical rendering, so an operand that cannot
    /// be normalised is one nothing stored could equal.
    #[test]
    fn a_time_operand_that_normalises_compares_against_its_canonical_text() {
        let time = parse_time("12:00:00,5Z").expect("the comma compatibility form");
        let sql = render(typed_column_condition(&Scalar::Time(time), CmpOp::Equal));
        assert!(sql.contains("value_string"), "{sql}");
    }
}
