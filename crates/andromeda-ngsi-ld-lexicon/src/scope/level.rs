//! One level of an NGSI-LD scope path (ETSI GS CIM 009 clause 4.18).

// `RegularGrammar` emits an undocumented error type and an undocumented owned
// accessor alongside the grammar type below, and an item-level allow does not
// reach macro-generated siblings. Fencing them here keeps the crate-level deny
// live for every hand-written item in the concept's other files.
#![expect(missing_docs, reason = "generated grammar items are documented by the source grammar")]

use static_regular_grammar::RegularGrammar;

/// The greatest number of characters one level of an NGSI-LD scope may carry.
///
/// Clause 4.18 bounds neither the number of levels a scope has nor the length
/// of one, so this is Andromeda's own bound rather than the specification's. It
/// is the widest label the label-path index every stored scope and every scope
/// query is answered through can hold, and a level beyond it is a request the
/// broker cannot serve rather than an internal failure raised by a database
/// cast.
///
/// The unit is characters and not bytes because that is what the index counts:
/// PostgreSQL bounds an `ltree` label by `LTREE_LABEL_MAX_CHARS`, counted in
/// characters of the server encoding. A 1000-character level of two-byte
/// letters is 2000 bytes and casts to `ltree` unrefused, while a
/// 1001-character level of one-byte letters does not -- both verified against
/// PostgreSQL 18.3 on UTF-8, which answers the second with `label string is
/// too long. DETAIL: Label length is 1001, must be at most 1000`.
pub const LEVEL_CHARACTER_LIMIT: usize = 1000;

/// The number of characters `level` carries, when that is beyond
/// [`LEVEL_CHARACTER_LIMIT`].
///
/// `None` says the level is within the bound. The count is what a refusal
/// reports, so the caller learns by how much rather than only that it failed.
#[must_use]
pub fn level_characters_beyond_limit(level: &str) -> Option<usize> {
    let characters = level.chars().count();
    (characters > LEVEL_CHARACTER_LIMIT).then_some(characters)
}

/// A validated level name inside a scope path (clause 4.18).
///
/// A Unicode letter followed by Unicode letters, digits or underscores. This is
/// the same production `Scope` is built from, derived once so that a scope
/// pattern — where the wildcards `+` and `#` stand in for a level — can validate
/// the levels it does carry without a second, hand-written reading of the
/// character classes.
///
/// This is an unsized type. Use [`ScopeLevelBuf`] for owned values.
#[derive(RegularGrammar, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[grammar(
    file = "grammar/.generated/scope-level.abnf",
    sized(ScopeLevelBuf, derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash)),
    serde
)]
pub struct ScopeLevel(str);

#[cfg(test)]
mod tests {
    use crate::scope::level::{LEVEL_CHARACTER_LIMIT, ScopeLevel, level_characters_beyond_limit};

    #[test]
    fn a_level_starts_with_a_letter_and_continues_with_letters_digits_or_underscores() {
        for raw in ["Madrid", "Level_2", "M\u{00fc}nchen", "a1"] {
            assert!(ScopeLevel::new(raw).is_ok(), "{raw} must be accepted");
        }
    }

    /// The bound counts characters rather than bytes, which is the unit
    /// PostgreSQL bounds an `ltree` label in: a 1000-character level of
    /// two-byte letters is 2000 bytes and still casts, and one character past
    /// the bound does not whatever its letters weigh.
    #[test]
    fn the_character_bound_admits_the_widest_level_and_refuses_the_next() {
        pretty_assertions::assert_eq!(level_characters_beyond_limit(&"a".repeat(LEVEL_CHARACTER_LIMIT)), None);
        pretty_assertions::assert_eq!(level_characters_beyond_limit(&"\u{00fc}".repeat(LEVEL_CHARACTER_LIMIT)), None);
        pretty_assertions::assert_eq!(level_characters_beyond_limit(&"a".repeat(LEVEL_CHARACTER_LIMIT + 1)), Some(1001));
    }

    #[test]
    fn a_level_outside_the_grammar_is_rejected() {
        for raw in ["", "1Floor", "_hidden", "A-B", "A B", "A/B"] {
            assert!(ScopeLevel::new(raw).is_err(), "{raw} must be rejected");
        }
    }
}
