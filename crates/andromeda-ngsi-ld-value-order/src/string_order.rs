//! How two text strings order under a clause 4.9 Query Term.
//!
//! Clause 4.9 has text strings compared following IETF RFC 8259 section 8.3,
//! which makes equality code unit identity, and says implementations "should
//! support the Unicode Collation Algorithm (UCA)". Two strings therefore order
//! by one of two relations: code point order, which decides equality, and the
//! CLDR root collation, the order clause 4.23.1 makes the default for strings
//! (ICU "root", `und-x-icu`). Two strings the collation finds equal but whose
//! code points differ order by code point, as a deterministic collation in the
//! store breaks the same tie, so the store and an in-memory evaluation agree.

use icu_collator::{CollatorBorrowed, options::CollatorOptions};
use std::cmp::Ordering;

/// The CLDR root collation at its default strength.
static ROOT_COLLATION: CollatorBorrowed<'static> = CollatorBorrowed::new_root(CollatorOptions::default());

/// The relation two text strings are ordered by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StringRelation {
    /// Code point order, under which two strings are equal only when they are
    /// the same sequence of code points.
    CodePoint,
    /// The CLDR root collation, its ties broken by code point.
    Collated,
}

/// The order of `lhs` against `rhs` under `relation`.
#[must_use]
pub fn string_order(lhs: &str, rhs: &str, relation: StringRelation) -> Ordering {
    match relation {
        StringRelation::CodePoint => lhs.cmp(rhs),
        StringRelation::Collated => ROOT_COLLATION.compare(lhs, rhs).then_with(|| lhs.cmp(rhs)),
    }
}

#[cfg(test)]
mod tests {
    use crate::string_order::{StringRelation, string_order};
    use pretty_assertions::assert_eq;
    use std::cmp::Ordering;

    /// Code point order puts every upper-case Latin letter before every
    /// lower-case one; the UCA orders by letter first.
    #[test]
    fn an_ordering_compares_letters_before_case() {
        assert_eq!(string_order("B", "a", StringRelation::Collated), Ordering::Greater);
        assert_eq!(string_order("a", "B", StringRelation::Collated), Ordering::Less);
    }

    /// Under the UCA an accented letter sorts beside its base letter, not after
    /// every unaccented one.
    #[test]
    fn an_ordering_places_an_accented_letter_beside_its_base_letter() {
        assert_eq!(string_order("\u{e9}", "f", StringRelation::Collated), Ordering::Less);
    }

    /// RFC 8259 section 8.3 makes String equality code unit identity.
    #[test]
    fn an_equality_compares_code_points() {
        assert_eq!(string_order("B", "a", StringRelation::CodePoint), Ordering::Less);
        assert_eq!(string_order("a", "a", StringRelation::CodePoint), Ordering::Equal);
    }

    /// Precomposed and decomposed spellings are canonically equivalent, which
    /// the collation finds equal; code point order breaks the tie.
    #[test]
    fn canonically_equivalent_strings_order_by_code_point() {
        assert_eq!(string_order("\u{e9}", "e\u{301}", StringRelation::Collated), Ordering::Greater);
    }
}
