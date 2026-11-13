//! What a query's page points at its neighbours from: the walk its pointers
//! belong to and the request they rebuild.
//!
//! A query answered on the query-string binding is walked by its own query
//! string. A query sent by POST is held by the broker and walked through the
//! GET binding of the same query by its digest. Either way the page decides its
//! pointers here, once, so every query binding points alike.

use crate::{
    link_target::PointedRequest,
    page_link_targets::PageLinkTargets,
    page_walk::PageWalk,
    pagination::{PageOutcome, page_link_targets},
};
use andromeda_ngsi_ld_query::page_window::PageWindow;
use andromeda_ngsi_ld_request::pagination::{page_context::PageContext, page_pointers::PagePointers, query_digest::QueryDigest};

/// The walk a query's page belongs to, and the request its pointers rebuild.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PagePointing {
    /// What the pointers are built over, if the page has any.
    pointers: PagePointers,
    /// The `@context` a query-string walk is read under, which every pointer
    /// of one carries (clause 5.5.9.1).
    context: PageContext,
    /// The digest of the query a query-string walk answers, or the digest of the
    /// posted query the walk names.
    query: QueryDigest,
    /// The request every pointer rebuilds, as a client reaches it.
    pointed: PointedRequest,
}

impl PagePointing {
    /// The pointing of a page built over `pointers`, of a walk read under
    /// `context` for `query`, whose pointers rebuild `pointed`.
    #[must_use]
    pub const fn new(pointers: PagePointers, context: PageContext, query: QueryDigest, pointed: PointedRequest) -> PagePointing {
        PagePointing {
            pointers,
            context,
            query,
            pointed,
        }
    }

    /// The clause 6.3.10 pointers of a page cut by `window` that returned
    /// `outcome`.
    #[must_use]
    pub fn targets(&self, window: &PageWindow, outcome: PageOutcome) -> PageLinkTargets {
        match self.pointers {
            PagePointers::Absent => PageLinkTargets::default(),
            PagePointers::OverRequestUri => page_link_targets(
                &self.pointed,
                PageWalk::QueryString {
                    context: &self.context,
                    query: &self.query,
                },
                window,
                outcome,
            ),
            PagePointers::OverPostedQuery => page_link_targets(&self.pointed, PageWalk::PostedQuery(&self.query), window, outcome),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{link_target::PointedRequest, page_pointing::PagePointing, pagination::PageOutcome};
    use andromeda_http_headers::link::relation_type::RelationType;
    use andromeda_ngsi_ld_query::{page_reach::PageReach, page_window::PageWindow};
    use andromeda_ngsi_ld_request::pagination::{page_context::PageContext, page_pointers::PagePointers, query_digest::QueryDigest};
    use axum::http::Uri;

    fn pointing(pointers: PagePointers, raw: &'static str) -> PagePointing {
        PagePointing::new(
            pointers,
            PageContext::try_from("https://example.com/a.jsonld").expect("an IRI"),
            QueryDigest::of(Some("type=Vehicle")),
            PointedRequest::new(None, Uri::from_static(raw)),
        )
    }

    fn continuing() -> PageOutcome {
        PageOutcome {
            total: None,
            reach: PageReach::Continues,
            boundary: None,
        }
    }

    #[test]
    fn a_query_string_walk_carries_its_context_and_a_posted_walk_its_digest() {
        let window = PageWindow::offset(2, 0);
        let query_string = pointing(PagePointers::OverRequestUri, "/ngsi-ld/v1/entities?type=Vehicle&limit=2").targets(&window, continuing());
        let next = query_string.target(&RelationType::Next).expect("a next pointer");
        assert!(next.contains("pageContext=") && !next.contains("postedQuery="), "{next}");
        let posted = pointing(PagePointers::OverPostedQuery, "/ngsi-ld/v1/entities?limit=2").targets(&window, continuing());
        let next = posted.target(&RelationType::Next).expect("a next pointer");
        assert!(next.contains("postedQuery=") && !next.contains("pageContext="), "{next}");
    }

    #[test]
    fn a_response_without_pointers_points_nowhere() {
        let targets = pointing(PagePointers::Absent, "/ngsi-ld/v1/entityMaps").targets(&PageWindow::offset(2, 0), continuing());
        assert!(targets.is_empty());
    }
}
