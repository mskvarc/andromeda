//! Which HTTP binding carried one "Query Entities" (clause 5.7.2).
//!
//! Clause 6.23.1 says of the POST binding that "the behaviour of this clause
//! mirrors the one in clause 6.4.3.2, which performs the 'Query Entity'
//! operation [...] by means of a GET method": it is the same operation, and
//! only the decoding differs. What nonetheless follows from which binding
//! decoded a query (how it is forwarded, what its pages point at, and whether
//! the broker holds it for those pointers) is a property of the binding rather
//! than of the query, so it is read off this one value.

use andromeda_federation::{operation::ForwardingOperation, request::QueryDelivery};
use andromeda_http_response::page_link_targets::PageLinkTargets;
use andromeda_ngsi_ld_request::pagination::page_pointers::PagePointers;
use andromeda_posted_query::{error::PostedQueryError, posted_query::PostedQuery, service::PostedQueryService};

/// The HTTP binding one Query Entities request arrived on.
#[derive(Debug, Clone)]
pub(crate) enum QueryBinding {
    /// `GET /entities` (clause 6.4.3.2): the whole query is in the request URI.
    QueryString,
    /// `POST /entityOperations/query` (clause 6.23.3.1): the query is in a
    /// clause 5.2.23 body, which clause 6.23.1 offers so that a client need not
    /// "end up assembling very long URLs". The broker holds it for the
    /// pointers of a paged result.
    PostedBody(Box<PostedQuery>),
    /// `GET /entities` following a pointer of a POST query: the query is the
    /// one the broker holds, run as the POST binding runs it.
    PostedReference,
}

impl QueryBinding {
    /// The operation a Context Source Registration has to declare for a query
    /// on this binding to be forwarded to it (Table 5.2.9-1 `operations`).
    ///
    /// The two bindings are separately registrable: `queryEntity` names the GET
    /// resource and `queryBatch` the POST one, so a registration that declares
    /// only one of them answers only that one. A followed pointer of a POST
    /// query runs the posted body, a selection the GET resource may not express,
    /// so it is forwarded as the POST was.
    pub(crate) const fn forwarding_operation(&self) -> ForwardingOperation {
        match self {
            QueryBinding::QueryString => ForwardingOperation::QueryEntity,
            QueryBinding::PostedBody(_) | QueryBinding::PostedReference => ForwardingOperation::QueryBatch,
        }
    }

    /// The binding a forwarded query prefers to mirror (clause 6.23.1): a GET
    /// query prefers `queryEntity`, a posted query `queryBatch`.
    pub(crate) const fn delivery(&self) -> QueryDelivery {
        match self {
            QueryBinding::QueryString => QueryDelivery::Query,
            QueryBinding::PostedBody(_) | QueryBinding::PostedReference => QueryDelivery::Batch,
        }
    }

    /// What a clause 6.3.10 pointer to the neighbouring pages is built over.
    ///
    /// Clause 6.3.10 requires the Link Target to be "a URI-reference that could
    /// be dereferenced by an NGSI-LD Client to retrieve the next page", and
    /// clause 5.5.9.1 that the pointers "contain all the parameters needed to
    /// allow NGSI-LD Clients to retrieve the next and previous page, without
    /// further interactions with the API". The POST binding's query is in its
    /// body, which no URI-reference carries, so its pointers name the query the
    /// broker holds.
    pub(crate) const fn page_pointers(&self) -> PagePointers {
        match self {
            QueryBinding::QueryString => PagePointers::OverRequestUri,
            QueryBinding::PostedBody(_) | QueryBinding::PostedReference => PagePointers::OverPostedQuery,
        }
    }

    /// Hold a posted query whose page points at another page, so the pointers
    /// can be followed.
    ///
    /// A query whose result fits on one page is not held, and following a
    /// pointer holds nothing again: the lifetime counts from the last POST.
    ///
    /// # Errors
    ///
    /// The posted-query store's failure to hold the query.
    pub(crate) async fn hold_for(&self, links: &PageLinkTargets, posted_queries: &PostedQueryService) -> Result<(), PostedQueryError> {
        match self {
            QueryBinding::PostedBody(posted) if !links.is_empty() => posted_queries.hold(posted).await,
            QueryBinding::QueryString | QueryBinding::PostedBody(_) | QueryBinding::PostedReference => Ok(()),
        }
    }
}
