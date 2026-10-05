//! The identifiers one traversal level asks for.

use crate::edge::{LinkedEdge, RemoteEligibility};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use indexmap::{IndexMap, IndexSet};

/// One identifier a level asks for, with whether it may be fetched from
/// outside the local store (clause 4.5.23.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedCandidate {
    /// The Linked Entity's identifier.
    pub id: Identifier,
    /// Whether any naming Relationship annotated it with an `objectType`.
    pub eligibility: RemoteEligibility,
}

/// Every not yet visited target `edges` name, once each, in first-encounter
/// order, eligible when any naming edge is.
#[must_use]
pub fn candidates_of<'e>(edges: impl IntoIterator<Item = &'e LinkedEdge>, visited: &IndexSet<Identifier>) -> Vec<LinkedCandidate> {
    let mut candidates: IndexMap<&Identifier, RemoteEligibility> = IndexMap::new();
    for edge in edges.into_iter().filter(|edge| !visited.contains(&edge.target)) {
        candidates
            .entry(&edge.target)
            .and_modify(|eligibility| *eligibility = eligibility.either(edge.eligibility))
            .or_insert(edge.eligibility);
    }
    candidates
        .into_iter()
        .map(|(id, eligibility)| LinkedCandidate {
            id: id.clone(), // clone: the candidate outlives the edges it was read from
            eligibility,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::{
        candidate::{LinkedCandidate, candidates_of},
        edge::{LinkedEdge, RemoteEligibility},
    };
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use indexmap::IndexSet;
    use pretty_assertions::assert_eq;

    fn id(raw: &str) -> Identifier {
        format!("urn:ngsi-ld:T:{raw}").parse().expect("valid URI")
    }

    fn edge(target: &str, eligibility: RemoteEligibility) -> LinkedEdge {
        LinkedEdge {
            from: id("A"),
            attribute: "linksTo".parse().expect("valid attribute name"),
            target: id(target),
            eligibility,
        }
    }

    fn candidate(raw: &str, eligibility: RemoteEligibility) -> LinkedCandidate {
        LinkedCandidate { id: id(raw), eligibility }
    }

    #[test]
    fn an_id_named_twice_keeps_its_first_position() {
        let edges = [
            edge("B", RemoteEligibility::LocalOnly),
            edge("C", RemoteEligibility::LocalOnly),
            edge("B", RemoteEligibility::LocalOnly),
        ];
        assert_eq!(
            candidates_of(&edges, &IndexSet::new()),
            vec![candidate("B", RemoteEligibility::LocalOnly), candidate("C", RemoteEligibility::LocalOnly)]
        );
    }

    /// Clause 4.5.23.1: an `objectType` on any naming Relationship declares
    /// the target an Internal Linked Entity.
    #[test]
    fn one_annotated_edge_makes_the_candidate_eligible() {
        let edges = [edge("B", RemoteEligibility::LocalOnly), edge("B", RemoteEligibility::Eligible)];
        assert_eq!(candidates_of(&edges, &IndexSet::new()), vec![candidate("B", RemoteEligibility::Eligible)]);
    }

    #[test]
    fn visited_ids_are_never_candidates() {
        let edges = [edge("B", RemoteEligibility::LocalOnly), edge("C", RemoteEligibility::LocalOnly)];
        let visited: IndexSet<Identifier> = [id("B")].into_iter().collect();
        assert_eq!(candidates_of(&edges, &visited), vec![candidate("C", RemoteEligibility::LocalOnly)]);
    }
}
