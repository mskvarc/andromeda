//! Whether a response can point at its neighbouring pages (clause 6.3.10).

/// Whether a response can carry the clause 6.3.10 pointers to its neighbouring
/// pages at all.
///
/// Clause 6.3.10 requires of the `next` pointer that "the Link Target shall be
/// a URI-reference that could be dereferenced by an NGSI-LD Client to retrieve
/// the next page of NGSI-LD Elements", and clause 5.5.9.1 that "the pointers to
/// NGSI-LD Elements shall contain all the parameters needed to allow NGSI-LD
/// Clients to retrieve the next and previous page, without further interactions
/// with the API". A query carried in a request body meets neither: the resource
/// it names answers only POST, so dereferencing the pointer cannot work, and
/// the pointer carries `limit` and `offset` without the query they page over.
/// Such a binding emits no pointer rather than one that misleads, and so it
/// can neither hand out nor continue a cursor walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PagePointers {
    /// The request URI carries the whole query, so a pointer built from it is
    /// dereferenceable and complete.
    OverRequestUri,
    /// The query reached the broker in the request body, which no URI-reference
    /// can carry. Clause 4.12's requirement to "flag NGSI-LD Clients when there
    /// are remaining NGSI-LD Elements to be traversed" is met on such a binding
    /// by the clause 6.3.13 `NGSILD-Results-Count` a counted query asks for.
    NotDereferenceable,
}
