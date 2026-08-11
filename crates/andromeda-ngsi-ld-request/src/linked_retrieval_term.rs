//! Which member of a request reached across a Relationship.

use strum::Display;

/// The request member that crossed into a Linked Entity.
///
/// Clause 4.9 gives `q` a `rel{…}` hop and clause 4.21 gives `pick`/`omit` a
/// nested sub-projection. A binding that admits neither — delete-by-query,
/// clause 5.6.21.4 — has to say which of the two it found, and the answer is a
/// fixed pair of cases rather than a sentence assembled at the failure site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
pub enum LinkedRetrievalTerm {
    /// A `q` predicate whose attribute path crosses a Relationship.
    #[strum(to_string = "the q filter traverses a Relationship")]
    Filter,
    /// A `pick` or `omit` term carrying a clause 4.21 sub-projection.
    #[strum(to_string = "a pick/omit term selects Linked Entity members")]
    Projection,
}

#[cfg(test)]
mod tests {
    use crate::linked_retrieval_term::LinkedRetrievalTerm;

    /// Each case states what was found in the request, so the rendered failure
    /// reads as one sentence with the clause reference around it.
    #[test]
    fn each_term_names_the_member_it_describes() {
        pretty_assertions::assert_eq!(LinkedRetrievalTerm::Filter.to_string(), "the q filter traverses a Relationship");
        pretty_assertions::assert_eq!(LinkedRetrievalTerm::Projection.to_string(), "a pick/omit term selects Linked Entity members");
    }
}
