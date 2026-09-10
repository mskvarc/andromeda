//! Which HTTP binding carried one "Query Entities" (clause 5.7.2).
//!
//! Clause 6.23.1 says of the POST binding that "the behaviour of this clause
//! mirrors the one in clause 6.4.3.2, which performs the 'Query Entity'
//! operation [...] by means of a GET method": it is the same operation, and
//! only the decoding differs. Two things nonetheless follow from which binding
//! decoded a query, and both are properties of the binding rather than of the
//! query, so they are read off this one value.

use andromeda_federation::operation::ForwardingOperation;
use andromeda_http_response::pagination::PagePointers;

/// The HTTP binding one Query Entities request arrived on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueryBinding {
    /// `GET /entities` (clause 6.4.3.2): the whole query is in the request URI.
    QueryString,
    /// `POST /entityOperations/query` (clause 6.23.3.1): the query is in a
    /// clause 5.2.23 body, which clause 6.23.1 offers so that a client need not
    /// "end up assembling very long URLs".
    PostedBody,
}

impl QueryBinding {
    /// The operation a Context Source Registration has to declare for a query
    /// on this binding to be forwarded to it (Table 5.2.9-1 `operations`).
    ///
    /// The two bindings are separately registrable: `queryEntity` names the GET
    /// resource and `queryBatch` the POST one, so a registration that declares
    /// only one of them answers only that one.
    pub(crate) const fn forwarding_operation(self) -> ForwardingOperation {
        match self {
            QueryBinding::QueryString => ForwardingOperation::QueryEntity,
            QueryBinding::PostedBody => ForwardingOperation::QueryBatch,
        }
    }

    /// Whether a clause 6.3.10 pointer to the neighbouring pages can be built
    /// from this binding's request URI at all.
    ///
    /// Clause 6.3.10 requires the Link Target to be "a URI-reference that could
    /// be dereferenced by an NGSI-LD Client to retrieve the next page", and
    /// clause 5.5.9.1 that the pointers "contain all the parameters needed to
    /// allow NGSI-LD Clients to retrieve the next and previous page, without
    /// further interactions with the API". The POST binding's query is in its
    /// body, so no URI-reference carries it and the resource it names answers
    /// no GET; it therefore points at nothing rather than at something that
    /// cannot be followed.
    pub(crate) const fn page_pointers(self) -> PagePointers {
        match self {
            QueryBinding::QueryString => PagePointers::OverRequestUri,
            QueryBinding::PostedBody => PagePointers::NotDereferenceable,
        }
    }
}
