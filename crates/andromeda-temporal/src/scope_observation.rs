//! What one scope observation of a temporal append intent records.

use andromeda_ngsi_ld_lexicon::scope::path::ScopePath;

/// One recorded statement about an entity's scope (clause 4.18).
///
/// The clause 4.18 grammar admits `urn:ngsi-ld:null` beside the hierarchical
/// paths, and the same clause reserves that spelling for a scope that has been
/// deleted. A deletion is therefore its own variant rather than a path with a
/// reserved name: scope history stores a deletion as a row carrying `deletedAt`
/// and no path at all, so a sentinel path would neither fit the `ltree` column
/// nor be distinguishable from a client that wrote that text. [`ScopePath`]
/// carries the other reading and admits no sentinel.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScopeObservation {
    /// The entity was placed in this validated scope path.
    Path(ScopePath),
    /// The entity's scope stopped applying.
    Deleted,
}
