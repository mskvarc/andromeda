//! What one context source contributed to the Entity Map of a read
//! (Table 5.2.39-2).

use andromeda_ngsi_ld_lexicon::uri::Uri;

/// What one registration's context source contributed to a read, for the
/// `EntityMap` of the request (Table 5.2.39-2): the entities it returned and
/// the map it answered with.
///
/// A contribution is what the map records about a source, so it lives beside
/// the map rather than beside the forwarding that produced it: a minted map
/// attributes each entity to the source that holds it, a reused one records the
/// map its source answered with, and the clause 4.3.6.7 pruning counts the
/// source as consulted for this page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceContribution {
    /// The registration whose source answered.
    pub registration: Uri,
    /// The entities it returned.
    pub entity_ids: Vec<Uri>,
    /// The `EntityMap` it used, when it returned one.
    pub linked_map: Option<Uri>,
}
