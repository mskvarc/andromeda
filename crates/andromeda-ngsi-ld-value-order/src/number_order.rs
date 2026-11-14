//! How a number orders under a clause 4.9 Query Term.
//!
//! Clause 4.6.3 makes a number an exact decimal, and a literal declared as
//! `xsd:float` or `xsd:double` may hold a value beyond the decimals. XSD orders
//! positive infinity above every finite number and negative infinity below,
//! and leaves not-a-number unordered, so it fails `==` and every ordering and
//! satisfies `!=`.

use andromeda_ngsi_ld_number::{JsonNumber, declared_number::NonFinite};
use std::cmp::Ordering;

/// A number a target value holds.
#[derive(Debug, Clone, Copy)]
pub enum HeldNumber<'a> {
    /// An exact decimal.
    Finite(&'a JsonNumber),
    /// A float or a double beyond the decimals.
    NonFinite(NonFinite),
}

/// How `held` orders against the finite number `operand`, `None` when it is
/// not-a-number.
#[must_use]
pub fn number_order(held: HeldNumber<'_>, operand: &JsonNumber) -> Option<Ordering> {
    match held {
        HeldNumber::Finite(number) => Some(number.cmp(operand)),
        HeldNumber::NonFinite(NonFinite::PositiveInfinity) => Some(Ordering::Greater),
        HeldNumber::NonFinite(NonFinite::NegativeInfinity) => Some(Ordering::Less),
        HeldNumber::NonFinite(NonFinite::NotANumber) => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::number_order::{HeldNumber, number_order};
    use andromeda_ngsi_ld_number::{JsonNumber, declared_number::NonFinite};
    use pretty_assertions::assert_eq;
    use std::cmp::Ordering;

    fn number(text: &str) -> JsonNumber {
        text.parse().expect("a number")
    }

    /// Clause 4.6.3: numbers compare as exact decimals, not as their text.
    #[test]
    fn finite_numbers_order_as_exact_decimals() {
        assert_eq!(number_order(HeldNumber::Finite(&number("2")), &number("10")), Some(Ordering::Less));
        assert_eq!(number_order(HeldNumber::Finite(&number("1.50")), &number("1.5")), Some(Ordering::Equal));
    }

    #[test]
    fn the_infinities_order_around_every_finite_number() {
        let huge = number("1e300");
        assert_eq!(number_order(HeldNumber::NonFinite(NonFinite::PositiveInfinity), &huge), Some(Ordering::Greater));
        assert_eq!(number_order(HeldNumber::NonFinite(NonFinite::NegativeInfinity), &huge), Some(Ordering::Less));
    }

    #[test]
    fn not_a_number_is_unordered() {
        assert_eq!(number_order(HeldNumber::NonFinite(NonFinite::NotANumber), &number("0")), None);
    }
}
