//! Attribute-name dictionary resolution for a compiled `q` filter.

use crate::attribute_name_id::AttributeNameId;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use std::collections::HashMap;

/// Attribute-name dictionary ids for the IRIs a compiled `q` filter mentions.
///
/// An IRI that is absent has no `attribute_names` row, and therefore no
/// `attribute_current` or `temporal_entity_attributes` row either, so its
/// predicates compile to constant `FALSE`.
///
/// There is deliberately no way to represent a *cached* miss. Resolution runs
/// per query execution against a cache that only ever stores hits, so an
/// attribute created after a filter was first compiled becomes visible on the
/// next execution instead of being pinned absent.
#[derive(Debug, Default, Clone)]
pub struct ResolvedAttributeNames {
    /// Expanded attribute name to dictionary id, hits only.
    ids: HashMap<AttributeName, AttributeNameId>,
}

impl ResolvedAttributeNames {
    /// Build from resolved `(expanded name, dictionary id)` pairs.
    pub fn from_pairs(pairs: impl IntoIterator<Item = (AttributeName, AttributeNameId)>) -> ResolvedAttributeNames {
        ResolvedAttributeNames {
            ids: pairs.into_iter().collect(),
        }
    }

    /// Dictionary id for an expanded attribute name, if the broker knows it.
    pub fn get(&self, attr: &AttributeName) -> Option<AttributeNameId> {
        self.ids.get(attr).copied()
    }

    /// Whether any attribute IRI resolved. Used to decide whether a query has
    /// to wait on the projection's read-your-writes barrier at all.
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }
}
