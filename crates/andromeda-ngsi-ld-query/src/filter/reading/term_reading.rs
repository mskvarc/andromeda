//! One Query Term with every clause 4.9 decision taken.

use crate::filter::reading::{hop::Hop, operand::Operand, terminal::Terminal, value_test::ValueTest};
use andromeda_ngsi_ld_lexicon::{attribute_iri::AttributeIri, attribute_name::AttributeName};

/// One Query Term, decided. Only lowering builds one, so every reading of a
/// term is one lowering has taken.
///
/// A term is existential over the instances of its attribute and over the
/// instances of each sub-attribute on the way, and two terms of one
/// conjunction may be satisfied by two different instances.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub struct TermReading {
    /// The linked-entity hops, outermost first.
    pub(super) hops: Vec<Hop>,
    /// The attribute of the Entity the last hop reaches, expanded.
    pub(super) attribute: AttributeIri,
    /// The sub-attributes walked below the attribute, outermost first.
    pub(super) sub_attributes: Vec<AttributeName>,
    /// What the path ends on at the last instance.
    pub(super) terminal: Terminal,
    /// What the term asks of the target.
    pub(super) test: Test,
}

/// What a term asks of the target its path reaches.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
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
    pub const fn attribute(&self) -> &AttributeIri {
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

    /// The term with the operands of a value list in canonical order, each
    /// once.
    pub(super) fn normal_form(mut self) -> TermReading {
        if let Test::Value(test) = &mut self.test {
            test.order_operands();
        }
        self
    }

    /// Joins `other` into this term when both assert an equality over the
    /// same targets, and gives `other` back otherwise.
    ///
    /// Both terms are existential over the same targets, so the term holding
    /// when either does is the equality with both value lists.
    pub(super) fn join_equality(&mut self, mut other: Box<TermReading>) -> Result<(), Box<TermReading>> {
        if !self.addresses_same_targets(&other) {
            return Err(other);
        }
        match (self.test.asserted_equality_mut(), other.test.asserted_equality_mut()) {
            (Some(operands), Some(joined)) => {
                operands.append(joined);
                operands.sort();
                operands.dedup();
                Ok(())
            }
            (Some(_) | None, None) | (None, Some(_)) => Err(other),
        }
    }

    /// Whether `other` reaches the targets this term reaches: the same hops,
    /// attribute, sub-attributes and terminal.
    fn addresses_same_targets(&self, other: &TermReading) -> bool {
        self.hops == other.hops && self.attribute == other.attribute && self.sub_attributes == other.sub_attributes && self.terminal == other.terminal
    }
}

impl Test {
    /// The operands of an asserted equality, `None` for any other test.
    const fn asserted_equality_mut(&mut self) -> Option<&mut Vec<Operand>> {
        match self {
            Test::Value(test) => test.asserted_equality_mut(),
            Test::Exists => None,
        }
    }
}
