//! One Query Term with every clause 4.9 decision taken.

use crate::filter::reading::{hop::Hop, terminal::Terminal, value_test::ValueTest};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;

/// One Query Term, decided. Only lowering builds one, so every reading of a
/// term is one lowering has taken.
///
/// A term is existential over the instances of its attribute and over the
/// instances of each sub-attribute on the way, and two terms of one
/// conjunction may be satisfied by two different instances.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermReading {
    /// The linked-entity hops, outermost first.
    pub(super) hops: Vec<Hop>,
    /// The attribute of the Entity the last hop reaches, expanded.
    pub(super) attribute: AttributeName,
    /// The sub-attributes walked below the attribute, outermost first.
    pub(super) sub_attributes: Vec<AttributeName>,
    /// What the path ends on at the last instance.
    pub(super) terminal: Terminal,
    /// What the term asks of the target.
    pub(super) test: Test,
}

/// What a term asks of the target its path reaches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Test {
    /// That the target element exists: "the matching Entities shall be those
    /// which define the target element ... regardless of any target value or
    /// object" (clause 4.9).
    Exists,
    /// That the target value passes a comparison.
    Value(ValueTest),
}

impl TermReading {
    /// The linked-entity hops, outermost first.
    #[must_use]
    pub const fn hops(&self) -> &[Hop] {
        self.hops.as_slice()
    }

    /// The attribute of the Entity the last hop reaches.
    #[must_use]
    pub const fn attribute(&self) -> &AttributeName {
        &self.attribute
    }

    /// The sub-attributes walked below the attribute, outermost first.
    #[must_use]
    pub const fn sub_attributes(&self) -> &[AttributeName] {
        self.sub_attributes.as_slice()
    }

    /// What the path ends on at the last instance.
    #[must_use]
    pub const fn terminal(&self) -> &Terminal {
        &self.terminal
    }

    /// What the term asks of the target.
    #[must_use]
    pub const fn test(&self) -> &Test {
        &self.test
    }
}
