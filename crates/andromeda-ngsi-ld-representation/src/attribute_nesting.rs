//! How deeply one written attribute may nest, and what an attribute nesting
//! deeper is refused with.
//!
//! ETSI GS CIM 009 v1.9.1 bounds no nesting: a Property value may be a JSON
//! object of any depth, a list may hold lists, and every attribute may carry
//! sub-attributes of its own (clause 4.5.2). Current state holds the
//! sub-attributes of an attribute instance as elements of one array, each
//! naming its parent by an `integer` position, so depth costs it one element
//! per level and no bound the value budgets reach. The queues and the temporal
//! history hold an instance in the same form, so the one JSON left in storage
//! is a structured value or a JSON literal in `value_object`, read back inside
//! one instance object with `serde_json`'s reader, which refuses a text nesting
//! more than [`READER_NESTING_LIMIT`] containers. The expansion wraps every
//! level of a value in a node and an array, so a write and the reads of what
//! it stored agree on what an attribute is only if the write is held to the
//! depth that read can take: [`AttributeNestingLimit::DECODABLE_CEILING`].
//!
//! The refusal is `BadRequestData`. Clause 6.3.2 keeps 413 for an input "too
//! large i.e. too many bytes", which a deep value of a few hundred bytes is not,
//! and clauses 5.7.1 and 5.7.2 raise `BadRequestData` for the one depth limit
//! the specification names, a "too deep query".

use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_number::json_value::READER_NESTING_LIMIT;
use derive_more::Display;
use std::num::NonZeroUsize;
use thiserror::Error;

/// Containers a stored structured value is read inside: the instance object
/// whose value member holds it.
const CONTAINERS_ABOVE_A_STORED_VALUE: usize = 1;

/// Containers below the deepest level of an attribute: the array of the
/// member holding its leaf, and the leaf's value object.
const CONTAINERS_BELOW_THE_DEEPEST_LEVEL: usize = 2;

/// Containers one level of an attribute takes at most in its expansion: a node
/// and the array of each of its members, a list object and its `@list` array,
/// or a sub-attribute's array and its instance.
const CONTAINERS_PER_LEVEL: usize = 2;

/// How deeply one attribute instance nests below itself, in the levels a client
/// writes: each sub-attribute, each node of a structured value, each list, and
/// each container inside a JSON literal is one level.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Display)]
#[display("{_0}")]
pub struct AttributeNesting(usize);

impl AttributeNesting {
    /// A nesting of `levels` levels.
    #[must_use]
    pub const fn new(levels: usize) -> AttributeNesting {
        AttributeNesting(levels)
    }

    /// The number of levels.
    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

/// The deepest nesting one attribute may have.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Display)]
#[display("{_0}")]
pub struct AttributeNestingLimit(NonZeroUsize);

impl AttributeNestingLimit {
    /// The deepest nesting every stored value of an attribute decodes at: the
    /// reader's limit, less the container a stored value is read inside and
    /// the containers below its deepest level, taken at the most containers a
    /// level costs. A sub-attribute is an element of its own, so its levels
    /// cost the stored JSON nothing.
    pub const DECODABLE_CEILING: AttributeNestingLimit = AttributeNestingLimit::decodable_ceiling();

    /// A limit of `levels` levels.
    #[must_use]
    pub const fn new(levels: NonZeroUsize) -> AttributeNestingLimit {
        AttributeNestingLimit(levels)
    }

    /// The number of levels the limit admits.
    #[must_use]
    pub const fn get(self) -> usize {
        self.0.get()
    }

    /// Whether `nesting` stays within the limit.
    #[must_use]
    pub const fn admits(self, nesting: AttributeNesting) -> bool {
        nesting.get() <= self.get()
    }

    /// Compute [`AttributeNestingLimit::DECODABLE_CEILING`].
    const fn decodable_ceiling() -> AttributeNestingLimit {
        let levels = READER_NESTING_LIMIT
            .saturating_sub(CONTAINERS_ABOVE_A_STORED_VALUE)
            .saturating_sub(CONTAINERS_BELOW_THE_DEEPEST_LEVEL)
            / CONTAINERS_PER_LEVEL;
        match NonZeroUsize::new(levels) {
            Some(levels) => AttributeNestingLimit(levels),
            None => AttributeNestingLimit(NonZeroUsize::MIN),
        }
    }
}

/// A written attribute nesting deeper than the limit admits.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[error("the attribute '{attribute}' nests deeper than the {limit} levels this broker accepts in one attribute; send a flatter value")]
pub struct AttributeNestingExceeded {
    /// The attribute nesting too deeply.
    pub attribute: AttributeName,
    /// The limit it passes.
    pub limit: AttributeNestingLimit,
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::attribute_nesting::{AttributeNesting, AttributeNestingExceeded, AttributeNestingLimit};
    use std::num::NonZeroUsize;

    fn limit(levels: usize) -> AttributeNestingLimit {
        AttributeNestingLimit::new(NonZeroUsize::new(levels).expect("a positive test limit"))
    }

    /// `serde_json` reads 127 containers; the instance object a stored value
    /// is read inside and the leaf take 3 of them, and each level 2.
    #[test]
    fn the_decodable_ceiling_is_sixty_two_levels() {
        pretty_assertions::assert_eq!(AttributeNestingLimit::DECODABLE_CEILING.get(), 62);
    }

    #[test]
    fn a_limit_admits_its_own_depth_and_refuses_one_more() {
        assert!(limit(32).admits(AttributeNesting::new(0)));
        assert!(limit(32).admits(AttributeNesting::new(32)));
        assert!(!limit(32).admits(AttributeNesting::new(33)));
    }

    /// The refusal names the attribute and the limit, and says what to send
    /// instead.
    #[test]
    fn the_refusal_names_the_attribute_and_the_limit() {
        let refusal = AttributeNestingExceeded {
            attribute: "https://example.org/reading".parse().expect("an attribute name"),
            limit: limit(32),
        };
        pretty_assertions::assert_eq!(
            refusal.to_string(),
            "the attribute 'https://example.org/reading' nests deeper than the 32 levels this broker accepts in one attribute; send a flatter value"
        );
    }
}
