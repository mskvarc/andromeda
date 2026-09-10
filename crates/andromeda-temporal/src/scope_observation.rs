//! What one scope observation of a temporal append intent records.

use andromeda_ngsi_ld_lexicon::scope::path::ScopePath;
use serde::{Deserialize, Serialize};

/// One recorded statement about an entity's scope (clause 4.18).
///
/// The clause 4.18 grammar admits `urn:ngsi-ld:null` beside the hierarchical
/// paths, and the same clause reserves that spelling for a scope that has been
/// deleted. A deletion is therefore its own variant rather than a path with a
/// reserved name: scope history stores a deletion as a row carrying `deletedAt`
/// and no path at all, so a sentinel path would neither fit the `ltree` column
/// nor be distinguishable from a client that wrote that text. [`ScopePath`]
/// carries the other reading and admits no sentinel.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScopeObservation {
    /// The entity was placed in this validated scope path.
    Path(ScopePath),
    /// The entity's scope stopped applying.
    Deleted,
}

#[cfg(test)]
mod tests {
    use crate::scope_observation::ScopeObservation;
    use andromeda_ngsi_ld_lexicon::scope::path::ScopePath;

    #[test]
    fn a_scope_observation_round_trips_through_its_durable_form() {
        let path = ScopeObservation::Path("/Madrid/Gardens".parse::<ScopePath>().expect("a valid scope path"));
        for observation in [path, ScopeObservation::Deleted] {
            let encoded = serde_json::to_string(&observation).expect("a scope observation serialises");
            let decoded: ScopeObservation = serde_json::from_str(&encoded).expect("a scope observation deserialises");
            pretty_assertions::assert_eq!(decoded, observation);
        }
    }
}
