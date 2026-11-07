//! The collation a clause 4.9 String ordering compares under.
//!
//! Clause 4.9 has text strings compared following IETF RFC 8259 section 8.3
//! (equality is code unit identity) and says implementations "should support
//! the Unicode Collation Algorithm (UCA)". Equality therefore stays byte
//! identity, which is what the stored String columns' `COLLATE "C"` gives, and
//! only an ordering (`>`, `>=`, `<`, `<=` and the bounds of a range) collates.
//! It collates under the ICU root collation, the order clause 4.23.1 makes the
//! default for strings, so `q=name>"a"` and `orderBy=name` agree.
//!
//! A `Date` or `Time` operand compares as the fixed-width canonical text the
//! String column holds it in, whose byte order is chronological order, so it
//! never collates here.

use andromeda_ngsi_ld_query::{
    filter::{predicate::CmpOp, scalar::Scalar},
    ordering::collation::IcuCollation,
};
use andromeda_postgresql_seaquery::collation::{PostgresCollation, collate};
use sea_query::SimpleExpr;

/// The collation `scalar` orders a stored string under with `op`, or `None`
/// when the comparison is not a String ordering.
#[must_use]
pub fn string_ordering_collation(scalar: &Scalar, op: CmpOp) -> Option<PostgresCollation> {
    match (scalar, op) {
        (Scalar::Str(_), CmpOp::Greater | CmpOp::GreaterEq | CmpOp::Less | CmpOp::LessEq) => Some(PostgresCollation::new(IcuCollation::ROOT)),
        (Scalar::Str(_), CmpOp::Equal | CmpOp::Unequal)
        | (Scalar::Uri(_) | Scalar::Number(_) | Scalar::Bool(_) | Scalar::DateTime(_) | Scalar::Date(_) | Scalar::Time(_), _) => None,
    }
}

/// `stored` under the collation [`string_ordering_collation`] picks, or
/// unchanged when it picks none.
#[must_use]
pub fn ordered_string(stored: SimpleExpr, scalar: &Scalar, op: CmpOp) -> SimpleExpr {
    match string_ordering_collation(scalar, op) {
        Some(collation) => collate(stored, &collation),
        None => stored,
    }
}

#[cfg(test)]
mod tests {
    use crate::q_predicate::string_ordering::{ordered_string, string_ordering_collation};
    use andromeda_ngsi_ld_query::filter::{
        predicate::CmpOp,
        scalar::{Scalar, parse_date, parse_time},
    };
    use andromeda_postgresql_seaquery::collation::PostgresCollation;
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query};

    fn render(scalar: &Scalar, op: CmpOp) -> String {
        Query::select()
            .expr(ordered_string(Expr::col(Alias::new("s")), scalar, op))
            .to_owned()
            .to_string(PostgresQueryBuilder)
    }

    /// Clause 4.9 recommends the UCA for comparing text strings, and clause
    /// 4.23.1 makes the ICU root collation the default string order.
    #[test]
    fn a_string_ordering_compares_under_the_icu_root_collation() {
        for op in [CmpOp::Greater, CmpOp::GreaterEq, CmpOp::Less, CmpOp::LessEq] {
            let collation = string_ordering_collation(&Scalar::Str("a".to_owned()), op).expect("a String ordering collates");
            assert_eq!(collation, PostgresCollation::new("und-x-icu"), "{op:?}");
            assert!(render(&Scalar::Str("a".to_owned()), op).contains(r#"("s") COLLATE "und-x-icu""#), "{op:?}");
        }
    }

    /// RFC 8259 section 8.3 makes String equality code unit identity, which a
    /// collation must not loosen.
    #[test]
    fn a_string_equality_is_not_collated() {
        for op in [CmpOp::Equal, CmpOp::Unequal] {
            assert!(string_ordering_collation(&Scalar::Str("a".to_owned()), op).is_none(), "{op:?}");
            assert!(!render(&Scalar::Str("a".to_owned()), op).contains("COLLATE"), "{op:?}");
        }
    }

    /// A date or time of day orders by its fixed-width canonical text, whose
    /// byte order is chronological, and a number, boolean or instant has an
    /// order of its own.
    #[test]
    fn a_non_string_operand_is_not_collated() {
        let date = parse_date("2024-01-15").expect("a date");
        let time = parse_time("12:00:00Z").expect("a time");
        for scalar in [Scalar::Date(date), Scalar::Time(time), Scalar::Number(1.into()), Scalar::Bool(true)] {
            assert!(string_ordering_collation(&scalar, CmpOp::Greater).is_none(), "{scalar:?}");
        }
    }
}
