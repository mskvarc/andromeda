//! What a response's pointers to its neighbouring pages are built over
//! (clause 6.3.10).

/// What a response builds the clause 6.3.10 pointers to its neighbouring pages
/// over, if it builds any.
///
/// Clause 6.3.10 requires of the `next` pointer that "the Link Target shall be
/// a URI-reference that could be dereferenced by an NGSI-LD Client to retrieve
/// the next page of NGSI-LD Elements", and clause 5.5.9.1 that "the pointers to
/// NGSI-LD Elements shall contain all the parameters needed to allow NGSI-LD
/// Clients to retrieve the next and previous page, without further interactions
/// with the API".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PagePointers {
    /// The request URI carries the whole query, so a pointer built from it is
    /// dereferenceable and complete.
    OverRequestUri,
    /// The query reached the broker in a request body, which no URI-reference
    /// can carry, so the broker holds it and every pointer names it by its key
    /// on the GET binding of the same query. Clause 5.5.9.2 allows the pointers
    /// to "be implemented in a completely opaque way".
    OverPostedQuery,
    /// The response is no page of a collection: it hands out no pointer, so a
    /// walk can neither start nor continue on it.
    Absent,
}
