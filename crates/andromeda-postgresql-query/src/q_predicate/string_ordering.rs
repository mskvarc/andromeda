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
    filter::reading::{comparator::Comparator, operand::Operand},
    ordering::collation::IcuCollation,
};
use andromeda_postgresql_seaquery::collation::{PostgresCollation, collate};
use sea_query::SimpleExpr;

/// The collation a stored string is compared with `operand` under by
/// `comparator`, or `None` when the comparison is not a String ordering.
#[must_use]
pub fn string_ordering_collation(operand: &Operand, comparator: Comparator) -> Option<PostgresCollation> {
    match (operand, comparator) {
        (Operand::Text(_), Comparator::Ordered(_)) => Some(PostgresCollation::new(IcuCollation::ROOT)),
        (Operand::Text(_), Comparator::Equal)
        | (Operand::Number(_) | Operand::Boolean(_) | Operand::DateTime(_) | Operand::Date(_) | Operand::Time(_), Comparator::Equal | Comparator::Ordered(_)) => {
            None
        }
    }
}

/// `stored` under the collation [`string_ordering_collation`] picks, or
/// unchanged when it picks none.
#[must_use]
pub fn ordered_string(stored: SimpleExpr, operand: &Operand, comparator: Comparator) -> SimpleExpr {
    match string_ordering_collation(operand, comparator) {
        Some(collation) => collate(stored, &collation),
        None => stored,
    }
}

#[cfg(test)]
mod tests {
    use crate::q_predicate::string_ordering::{ordered_string, string_ordering_collation};
    use andromeda_ngsi_ld_query::filter::{
        reading::{comparator::Comparator, operand::Operand, order_relation::OrderRelation},
        scalar::{parse_date, parse_time},
    };
    use andromeda_postgresql_seaquery::{assert_statement_snapshot, collation::PostgresCollation};
    use sea_query::{Alias, Expr, Query, SelectStatement};

    fn render(operand: &Operand, comparator: Comparator) -> SelectStatement {
        Query::select().expr(ordered_string(Expr::col(Alias::new("s")), operand, comparator)).to_owned()
    }

    fn text(value: &str) -> Operand {
        Operand::Text(value.to_owned())
    }

    /// Clause 4.9 recommends the UCA for comparing text strings, and clause
    /// 4.23.1 makes the ICU root collation the default string order.
    #[test]
    fn a_string_ordering_compares_under_the_icu_root_collation() {
        for (index, relation) in [
            OrderRelation::Greater,
            OrderRelation::GreaterOrEqual,
            OrderRelation::Less,
            OrderRelation::LessOrEqual,
        ]
        .into_iter()
        .enumerate()
        {
            let comparator = Comparator::Ordered(relation);
            let collation = string_ordering_collation(&text("a"), comparator).expect("a String ordering collates");
            assert_eq!(collation, PostgresCollation::new("und-x-icu"), "{relation:?}");
            assert_statement_snapshot!(
                format!("string-ordering-a-string-ordering-compares-under-the-icu-root-collation-{}", index + 1),
                render(&text("a"), comparator)
            );
        }
    }

    /// RFC 8259 section 8.3 makes String equality code unit identity, which a
    /// collation must not loosen.
    #[test]
    fn a_string_equality_is_not_collated() {
        assert!(string_ordering_collation(&text("a"), Comparator::Equal).is_none());
        assert_statement_snapshot!("string-ordering-a-string-equality-is-not-collated", render(&text("a"), Comparator::Equal));
    }

    /// A date or time of day orders by its fixed-width canonical text, whose
    /// byte order is chronological, and a number, boolean or instant has an
    /// order of its own.
    #[test]
    fn a_non_string_operand_is_not_collated() {
        let date = parse_date("2024-01-15").expect("a date");
        let time = parse_time("12:00:00Z").expect("a time");
        for operand in [Operand::Date(date), Operand::Time(time), Operand::Number(1.into()), Operand::Boolean(true)] {
            assert!(
                string_ordering_collation(&operand, Comparator::Ordered(OrderRelation::Greater)).is_none(),
                "{operand:?}"
            );
        }
    }
}
