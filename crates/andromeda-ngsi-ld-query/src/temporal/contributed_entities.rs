//! The entities a Context Source contributed a part of to a temporal query
//! under `splitEntities` (clause 5.7.4.4).

use andromeda_ngsi_ld_lexicon::identifier::Identifier;

/// How the local read renders a contributed entity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ContributedRendering {
    /// The local part is rendered like any other entity and merged with the
    /// parts the sources answered with.
    #[default]
    Local,
    /// The local part is not rendered: the entity is rendered from its merged
    /// instances instead, which is what an aggregated temporal representation
    /// of an entity held in several parts needs (clause 5.7.4.4, "aggregated
    /// values of Attribute instances").
    Merged,
}

/// The identifiers of every entity a Context Source answered a split temporal
/// query with.
///
/// Clause 5.7.4.4 applies the value, geo and scope filters to an entity only
/// after its parts from the sources and from this broker have been
/// aggregated. An entity no source answered with has no part outside this
/// broker in the queried interval, so its local part decides those filters
/// exactly; only the entities named here are exempt from them locally.
///
/// The identifiers are held sorted and without repetition, so membership is a
/// binary search and the order they are bound in is stable.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContributedEntities {
    /// The contributed identifiers, sorted by their text, each once.
    ids: Vec<Identifier>,
    /// How the local read renders them.
    rendering: ContributedRendering,
}

impl ContributedEntities {
    /// The contributed entities `ids`, rendered as `rendering` says.
    pub fn new(ids: impl IntoIterator<Item = Identifier>, rendering: ContributedRendering) -> ContributedEntities {
        let mut ids: Vec<Identifier> = ids.into_iter().collect();
        ids.sort_unstable_by(|left, right| left.as_str().cmp(right.as_str()));
        ids.dedup();
        ContributedEntities { ids, rendering }
    }

    /// Whether no source contributed any entity.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// Whether a source contributed a part of the entity `id`.
    #[must_use]
    pub fn contains(&self, id: &Identifier) -> bool {
        self.ids.binary_search_by(|held| held.as_str().cmp(id.as_str())).is_ok()
    }

    /// The contributed identifiers, sorted by their text.
    #[must_use]
    pub fn as_slice(&self) -> &[Identifier] {
        &self.ids
    }

    /// How the local read renders a contributed entity.
    #[must_use]
    pub const fn rendering(&self) -> ContributedRendering {
        self.rendering
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::temporal::contributed_entities::{ContributedEntities, ContributedRendering};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;

    fn id(raw: &str) -> Identifier {
        raw.parse().expect("valid identifier")
    }

    #[test]
    fn an_empty_set_contains_nothing() {
        let contributed = ContributedEntities::default();
        assert!(contributed.is_empty());
        assert!(!contributed.contains(&id("urn:ngsi-ld:Vehicle:1")));
        pretty_assertions::assert_eq!(contributed.rendering(), ContributedRendering::Local);
    }

    #[test]
    fn membership_is_by_identifier() {
        let contributed = ContributedEntities::new(
            [id("urn:ngsi-ld:Vehicle:2"), id("urn:ngsi-ld:Vehicle:1"), id("urn:ngsi-ld:Vehicle:2")],
            ContributedRendering::Merged,
        );
        assert!(contributed.contains(&id("urn:ngsi-ld:Vehicle:1")));
        assert!(contributed.contains(&id("urn:ngsi-ld:Vehicle:2")));
        assert!(!contributed.contains(&id("urn:ngsi-ld:Vehicle:3")));
        pretty_assertions::assert_eq!(contributed.as_slice(), &[id("urn:ngsi-ld:Vehicle:1"), id("urn:ngsi-ld:Vehicle:2")]);
        pretty_assertions::assert_eq!(contributed.rendering(), ContributedRendering::Merged);
    }
}
