//! The terms of a context whose container reshapes the members compacted
//! under them, and how such a member is read back.
//!
//! JSON-LD 1.1 compaction writes a value under a term whose definition names a
//! container in the shape that container gives (JSON-LD 1.1 section 4.6): an
//! array of one for a `@set`, an object keyed by index, identifier or type for
//! a map container, and the members of a `@nest` alias moved into an object of
//! their own. A user `@context` may define any of them (clause 5.5.7), and the
//! data is the same whichever shape compaction gave it, so the compact model
//! reads a member written under such a term as compaction writes it without
//! the container. The containers the Core `@context` gives its own terms (the
//! `@language` map of `languageMap`) are the NGSI-LD shape itself and are not
//! among these terms.

use std::collections::HashMap;

/// What a term's container makes of the members compacted under it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TermContainer {
    /// `@set`: a single value is written as an array of one.
    Set,
    /// `@index`, `@id`, `@type` or `@graph`: the values are written as the
    /// values of an object keyed by what the container names.
    Map,
    /// An alias of `@nest`: the members nested under it belong to the object
    /// it is a member of.
    Nest,
}

/// The terms of a context whose container reshapes what is compacted under
/// them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContainerTerms(HashMap<Box<str>, TermContainer>);

impl ContainerTerms {
    /// No term reshapes what is compacted under it.
    #[must_use]
    pub fn none() -> ContainerTerms {
        ContainerTerms(HashMap::new())
    }

    /// Record that `container` reshapes what is compacted under `term`.
    pub fn contain(&mut self, term: &str, container: TermContainer) {
        self.0.insert(Box::from(term), container);
    }

    /// Whether no term reshapes what is compacted under it.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The container of the term `name`, when it reshapes what is compacted
    /// under it.
    #[must_use]
    pub fn container_of(&self, name: &str) -> Option<TermContainer> {
        self.0.get(name).copied()
    }
}

#[cfg(test)]
mod tests {
    use crate::container_terms::{ContainerTerms, TermContainer};

    #[test]
    fn a_term_is_read_by_the_container_recorded_for_it() {
        let mut terms = ContainerTerms::none();
        assert!(terms.is_empty());
        terms.contain("tps", TermContainer::Set);
        terms.contain("n", TermContainer::Nest);
        pretty_assertions::assert_eq!(terms.container_of("tps"), Some(TermContainer::Set));
        pretty_assertions::assert_eq!(terms.container_of("n"), Some(TermContainer::Nest));
        pretty_assertions::assert_eq!(terms.container_of("speed"), None);
    }
}
