//! The hierarchical form of an NGSI-LD scope (ETSI GS CIM 009 clause 4.18).

use crate::scope::{
    grammar::Scope,
    level::{LEVEL_CHARACTER_LIMIT, level_characters_beyond_limit},
};
use andromeda_ngsi_ld_vocabulary::NULL_URI;
use compact_str::CompactString;
use derive_more::{AsRef, Display, Into};
use serde::{Deserialize, Serialize};
use std::str::{FromStr, Split};
use thiserror::Error;

/// The separator between two levels of a scope path (clause 4.18).
const LEVEL_SEPARATOR: char = '/';

/// The bytes a label-path index spends on one level beside the level itself:
/// PostgreSQL prefixes every `ltree` label with its length as a `uint16`.
const LEVEL_LENGTH_BYTES: usize = 2;

/// The greatest size, in bytes, a scope path may occupy in a label-path index.
///
/// Clause 4.18 bounds neither the number of levels a scope has nor its total
/// length, so this is Andromeda's own bound. It exists because
/// [`LEVEL_CHARACTER_LIMIT`] alone does not keep a path storable: a `GiST` index
/// row must fit in the 8191 bytes an index page admits, and the row a
/// label-path index builds for one path runs to roughly twice the path's
/// encoded size. Verified against PostgreSQL 18.3, where a path of five
/// 1000-character levels is refused with "index row requires 10136 bytes,
/// maximum size is 8191" while four of them are stored. Refusing the path here
/// answers the request as bad input rather than as the internal failure that
/// index insert would raise.
///
/// The bound also settles the label count: PostgreSQL admits 65535 labels in
/// one `ltree`, and clause 4.18 gives every level at least one character, so a
/// path is past this bound long before it reaches that many levels.
const PATH_ENCODED_BYTE_LIMIT: usize = 3000;

/// Why a string does not name a scope path.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ScopePathError {
    /// The string is outside the clause 4.18 scope grammar.
    #[error("'{0}' does not satisfy the NGSI-LD scope grammar")]
    Malformed(String),
    /// The string is the NGSI-LD Null, which names a deleted scope rather than a place (ETSI GS CIM 009 clause 4.18).
    #[error("'{null}' is the NGSI-LD Null, which names only a deleted scope, not a scope path", null = NULL_URI.as_str())]
    NullSentinel,
    /// One level of the path is longer than [`LEVEL_CHARACTER_LIMIT`].
    #[error("a scope level of {0} characters exceeds the {LEVEL_CHARACTER_LIMIT} characters a scope level may carry")]
    LevelTooLong(usize),
    /// The whole path takes more index bytes than [`PATH_ENCODED_BYTE_LIMIT`].
    #[error("a scope path of {0} encoded bytes exceeds the {PATH_ENCODED_BYTE_LIMIT} bytes a scope path may occupy in a label-path index")]
    PathTooLong(usize),
}

/// A hierarchical NGSI-LD scope path (clause 4.18).
///
/// Clause 4.18 puts two different things under one production: the hierarchical
/// path this type carries, and the NGSI-LD Null, of which the same clause says
/// the special string "shall be only used and only appear in case of deleted
/// scopes". A deletion is not a place an entity can be, so it is not a value of
/// this type — code holding a `ScopePath` holds somewhere an entity is, which
/// is what a label-path index stores and a `scopeQ` of clause 4.19 matches.
/// [`Scope`] remains the wire grammar, admitting both.
#[derive(AsRef, Clone, Debug, Deserialize, Display, Eq, Hash, Into, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "String", into = "String")]
#[as_ref(str)]
#[display("{_0}")]
#[into(String)]
pub struct ScopePath(CompactString);

impl ScopePath {
    /// Borrow the path in the clause 4.18 spelling it was written in.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// The levels of the path, outermost first.
    ///
    /// Clause 4.18 makes the leading `/` optional and every scope absolute, so
    /// both spellings of one path yield the same levels.
    #[must_use]
    pub fn levels(&self) -> Split<'_, char> {
        levels_of(self.as_str())
    }
}

/// The levels of a scope path spelled `raw`, outermost first.
fn levels_of(raw: &str) -> Split<'_, char> {
    raw.strip_prefix(LEVEL_SEPARATOR).unwrap_or(raw).split(LEVEL_SEPARATOR)
}

/// The bytes a scope path spelled `raw` occupies in a label-path index: each
/// level's own bytes plus the length PostgreSQL prefixes it with.
fn encoded_bytes(raw: &str) -> usize {
    levels_of(raw).map(|level| level.len() + LEVEL_LENGTH_BYTES).sum()
}

// Validating parser: the grammar check, the NGSI-LD Null and the level bound
// are what make the newtype worth having, and no derive expresses a fallible
// parse.
impl FromStr for ScopePath {
    type Err = ScopePathError;

    fn from_str(raw: &str) -> Result<ScopePath, ScopePathError> {
        if raw == NULL_URI.as_str() {
            return Err(ScopePathError::NullSentinel);
        }
        Scope::new(raw).map_err(|_| ScopePathError::Malformed(raw.to_owned()))?;
        for level in levels_of(raw) {
            if let Some(characters) = level_characters_beyond_limit(level) {
                return Err(ScopePathError::LevelTooLong(characters));
            }
        }
        let encoded = encoded_bytes(raw);
        if encoded > PATH_ENCODED_BYTE_LIMIT {
            return Err(ScopePathError::PathTooLong(encoded));
        }
        Ok(ScopePath(CompactString::from(raw)))
    }
}

// Parsing lives in `FromStr` so borrowed call sites do not allocate; this
// adapter exists only because `#[serde(try_from = "String")]` requires it.
impl TryFrom<String> for ScopePath {
    type Error = ScopePathError;

    fn try_from(value: String) -> Result<ScopePath, ScopePathError> {
        value.parse()
    }
}

#[cfg(test)]
mod tests {
    use crate::scope::{
        level::LEVEL_CHARACTER_LIMIT,
        path::{PATH_ENCODED_BYTE_LIMIT, ScopePath, ScopePathError},
    };

    #[test]
    fn the_hierarchical_spellings_of_clause_4_18_are_scope_paths() {
        for raw in [
            "/Madrid",
            "Madrid",
            "/Madrid/Gardens/ParqueNorte",
            "/CompanyA/OrganizationB/UnitC",
            "/Floor1/Room_42",
        ] {
            pretty_assertions::assert_eq!(raw.parse::<ScopePath>().expect(raw).as_str(), raw);
        }
    }

    /// Clause 4.18: the special string "urn:ngsi-ld:null" "shall be only used
    /// and only appear in case of deleted scopes", so it names no place and is
    /// not a scope path even though the clause 4.18 grammar admits the spelling.
    #[test]
    fn the_ngsi_ld_null_is_not_a_scope_path() {
        pretty_assertions::assert_eq!(
            "urn:ngsi-ld:null".parse::<ScopePath>().expect_err("the null names a deletion"),
            ScopePathError::NullSentinel
        );
    }

    #[test]
    fn a_string_outside_the_clause_4_18_grammar_is_not_a_scope_path() {
        for raw in ["", "/", "/A/B/", "/A//B", "/1Floor", "/_hidden", "/A/B-C", "/A B"] {
            pretty_assertions::assert_eq!(
                raw.parse::<ScopePath>().expect_err(raw),
                ScopePathError::Malformed(raw.to_owned()),
                "{raw} is outside the grammar"
            );
        }
    }

    /// A level beyond the bound is refused by the parser rather than by the
    /// label-path index it would later be written to, so the request is
    /// answered as bad input instead of as an internal failure.
    #[test]
    fn a_level_beyond_the_character_bound_is_refused() {
        let longest = "a".repeat(LEVEL_CHARACTER_LIMIT);
        assert!(format!("/{longest}").parse::<ScopePath>().is_ok(), "the bound itself is admitted");
        pretty_assertions::assert_eq!(
            format!("/{longest}a").parse::<ScopePath>().expect_err("one character past the bound"),
            ScopePathError::LevelTooLong(LEVEL_CHARACTER_LIMIT + 1)
        );
    }

    /// The level bound counts characters, not bytes, because that is the unit
    /// PostgreSQL bounds an `ltree` label in: a level of 1000 two-byte letters
    /// weighs 2000 bytes and is stored, as PostgreSQL 18.3 confirms.
    #[test]
    fn the_level_bound_counts_characters_rather_than_bytes() {
        assert!(format!("/{}", "\u{00fc}".repeat(LEVEL_CHARACTER_LIMIT)).parse::<ScopePath>().is_ok());
    }

    /// A path of levels each within the character bound can still be too wide
    /// for the index row that holds it, so the encoded size is bounded too.
    /// Three levels of 1000 two-byte letters weigh 6006 encoded bytes, which
    /// no label-path index row admits.
    #[test]
    fn a_path_within_the_level_bound_is_refused_when_its_encoded_size_is_not() {
        let level = "\u{00fc}".repeat(LEVEL_CHARACTER_LIMIT);
        let raw = format!("/{level}/{level}/{level}");
        pretty_assertions::assert_eq!(
            raw.parse::<ScopePath>().expect_err("three of the widest levels do not fit one index row"),
            ScopePathError::PathTooLong(3 * (2 * LEVEL_CHARACTER_LIMIT + 2))
        );
    }

    /// The encoded size counts the two bytes PostgreSQL prefixes every level
    /// with, so a path of many short levels is bounded by its level count as
    /// much as by its letters.
    #[test]
    fn the_encoded_bound_counts_the_length_prefix_of_every_level() {
        // Each "/ab" level costs its two letters plus the two-byte length.
        let levels = PATH_ENCODED_BYTE_LIMIT / 4;
        let within = (0..levels).map(|_| "/ab").collect::<String>();
        pretty_assertions::assert_eq!(within.parse::<ScopePath>().expect("the bound itself is admitted").as_str(), within);
        let beyond = format!("{within}/ab");
        pretty_assertions::assert_eq!(
            beyond.parse::<ScopePath>().expect_err("one level past the bound"),
            ScopePathError::PathTooLong(PATH_ENCODED_BYTE_LIMIT + 4)
        );
    }

    /// An ordinary clause 4.18 scope is nowhere near either bound.
    #[test]
    fn the_hierarchical_examples_of_clause_4_18_are_far_within_the_encoded_bound() {
        for raw in ["/Madrid", "/Madrid/Gardens/ParqueNorte", "/CompanyA/OrganizationB/UnitC"] {
            assert!(raw.parse::<ScopePath>().is_ok(), "{raw}");
        }
    }

    #[test]
    fn a_path_yields_the_same_levels_with_and_without_its_leading_slash() {
        let with = "/Madrid/Gardens".parse::<ScopePath>().expect("a valid scope path");
        let without = "Madrid/Gardens".parse::<ScopePath>().expect("a valid scope path");
        pretty_assertions::assert_eq!(with.levels().collect::<Vec<&str>>(), vec!["Madrid", "Gardens"]);
        pretty_assertions::assert_eq!(with.levels().collect::<Vec<&str>>(), without.levels().collect::<Vec<&str>>());
    }

    #[test]
    fn a_scope_path_round_trips_through_its_wire_form() {
        let path = "/Madrid/Gardens".parse::<ScopePath>().expect("a valid scope path");
        let encoded = serde_json::to_string(&path).expect("a scope path serialises");
        pretty_assertions::assert_eq!(encoded, "\"/Madrid/Gardens\"");
        pretty_assertions::assert_eq!(serde_json::from_str::<ScopePath>(&encoded).expect("a scope path deserialises"), path);
    }

    #[test]
    fn the_wire_form_refuses_a_value_that_is_not_a_scope_path() {
        assert!(serde_json::from_str::<ScopePath>("\"urn:ngsi-ld:null\"").is_err());
        assert!(serde_json::from_str::<ScopePath>("\"//invalid\"").is_err());
    }
}
