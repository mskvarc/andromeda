//! How a target value is compared against one operand.

use crate::filter::reading::{
    operand::{Comparison, Operand},
    order_relation::OrderRelation,
};
use andromeda_ngsi_ld_number::declared_number::NonFinite;
use andromeda_ngsi_ld_value_order::{
    number_order::{HeldNumber, number_order},
    string_order::string_order,
};
use std::cmp::Ordering;

/// One comparison of a target value against one operand: equality, the test
/// of `==`, `!=` and each value of a list, or an order, the test of an
/// ordering operator and of each bound of a range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Comparator {
    /// Equal to the operand.
    Equal,
    /// Ordered against the operand by the relation.
    Ordered(OrderRelation),
}

impl Comparator {
    /// Which operators the comparison stands after.
    #[must_use]
    pub const fn comparison(self) -> Comparison {
        match self {
            Comparator::Equal => Comparison::Equality,
            Comparator::Ordered(_) => Comparison::Order,
        }
    }

    /// Whether a target ordered `ordering` against the operand passes the
    /// comparison.
    #[must_use]
    pub const fn holds(self, ordering: Ordering) -> bool {
        match self {
            Comparator::Equal => ordering.is_eq(),
            Comparator::Ordered(relation) => relation.holds(ordering),
        }
    }

    /// Whether the comparison holds between the text strings `target` and
    /// `operand`, ordered by the relation [`Comparison::string_relation`]
    /// gives it.
    #[must_use]
    pub fn holds_for_strings(self, target: &str, operand: &str) -> bool {
        self.holds(string_order(target, operand, self.comparison().string_relation()))
    }

    /// Whether `value`, a number beyond the decimals, passes the comparison
    /// with `operand`, in the order the number primitive gives it against a
    /// finite number: never for an operand that is no number, and never for
    /// a not-a-number, which no number orders against.
    #[must_use]
    pub fn holds_beyond_decimals(self, value: NonFinite, operand: &Operand) -> bool {
        operand
            .number()
            .and_then(|number| number_order(HeldNumber::NonFinite(value), number))
            .is_some_and(|ordering| self.holds(ordering))
    }
}

#[cfg(test)]
mod tests {
    use crate::filter::reading::{comparator::Comparator, operand::Operand, order_relation::OrderRelation};
    use andromeda_ngsi_ld_number::declared_number::NonFinite;

    /// Equality compares code points and an order collates, as clause 4.9
    /// compares text strings: a precomposed `é` and `e` with a combining acute
    /// are canonically equivalent but two sequences of code points.
    #[test]
    fn strings_compare_by_code_point_for_equality_and_by_the_collation_for_an_order() {
        assert!(Comparator::Equal.holds_for_strings("CEL", "CEL"));
        assert!(!Comparator::Equal.holds_for_strings("cel", "CEL"));
        assert!(!Comparator::Equal.holds_for_strings("\u{e9}", "e\u{301}"));
        assert!(Comparator::Ordered(OrderRelation::Greater).holds_for_strings("\u{e9}", "e\u{301}"));
        assert!(Comparator::Ordered(OrderRelation::Less).holds_for_strings("\u{e9}", "f"));
        assert!(Comparator::Ordered(OrderRelation::LessOrEqual).holds_for_strings("a", "B"));
        assert!(!Comparator::Ordered(OrderRelation::GreaterOrEqual).holds_for_strings("a", "B"));
        assert!(Comparator::Ordered(OrderRelation::Less).holds_for_strings("a", "B"));
        assert!(Comparator::Ordered(OrderRelation::GreaterOrEqual).holds_for_strings("b", "b"));
        assert!(!Comparator::Ordered(OrderRelation::Greater).holds_for_strings("b", "b"));
    }

    /// XSD orders the infinities beyond every decimal and leaves not-a-number
    /// unordered, so no comparison but none holds for it.
    #[test]
    fn a_number_beyond_the_decimals_compares_as_xsd_orders_it() {
        let five = Operand::Number("5".parse().expect("a number"));
        let greater = Comparator::Ordered(OrderRelation::Greater);
        let less = Comparator::Ordered(OrderRelation::LessOrEqual);
        assert!(greater.holds_beyond_decimals(NonFinite::PositiveInfinity, &five));
        assert!(!greater.holds_beyond_decimals(NonFinite::NegativeInfinity, &five));
        assert!(less.holds_beyond_decimals(NonFinite::NegativeInfinity, &five));
        for comparator in [Comparator::Equal, greater, less] {
            assert!(!comparator.holds_beyond_decimals(NonFinite::NotANumber, &five), "{comparator:?}");
            assert!(
                !comparator.holds_beyond_decimals(NonFinite::PositiveInfinity, &Operand::Text("5".to_owned())),
                "{comparator:?}"
            );
        }
        assert!(!Comparator::Equal.holds_beyond_decimals(NonFinite::PositiveInfinity, &five));
    }
}
