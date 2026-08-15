//! One level of an NGSI-LD scope path (ETSI GS CIM 009 clause 4.18).

// `RegularGrammar` emits an undocumented error type and an undocumented owned
// accessor alongside the grammar type below, and an item-level allow does not
// reach macro-generated siblings. Fencing them here keeps the crate-level deny
// live for every hand-written item in the concept's other files.
#![expect(missing_docs, reason = "generated grammar items are documented by the source grammar")]

use static_regular_grammar::RegularGrammar;

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
    use crate::scope::level::ScopeLevel;

    #[test]
    fn a_level_starts_with_a_letter_and_continues_with_letters_digits_or_underscores() {
        for raw in ["Madrid", "Level_2", "M\u{00fc}nchen", "a1"] {
            assert!(ScopeLevel::new(raw).is_ok(), "{raw} must be accepted");
        }
    }

    #[test]
    fn a_level_outside_the_grammar_is_rejected() {
        for raw in ["", "1Floor", "_hidden", "A-B", "A B", "A/B"] {
            assert!(ScopeLevel::new(raw).is_err(), "{raw} must be rejected");
        }
    }
}
