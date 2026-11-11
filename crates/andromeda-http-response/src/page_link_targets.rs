//! The clause 6.3.10 pointers of one page, built before a response carries
//! them.
//!
//! A page's pointers are decided before they are written, so an operation can
//! act on whether the page points anywhere: a posted query is held only when
//! its result has another page to point at.

use crate::link_target::append_typed_links;
use andromeda_http_headers::{link::relation_type::RelationType, media_type::ResponseMediaType};
use axum::response::Response;

/// The relation and target of every pointer of one page.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageLinkTargets(Vec<(RelationType, String)>);

impl PageLinkTargets {
    /// The pointers `targets` names.
    pub(crate) const fn new(targets: Vec<(RelationType, String)>) -> PageLinkTargets {
        PageLinkTargets(targets)
    }

    /// Whether the page points at no other page.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Append the pointers to `response` as one `Link` header, each typed as
    /// `media_type` (clause 6.3.10); a page pointing nowhere adds nothing.
    ///
    /// The header is appended rather than inserted: a response rendered with
    /// the `@context` in the headers (clause 6.3.5) already carries a `Link`
    /// header, and replacing it would drop the context the client needs.
    pub fn append_to(self, response: &mut Response, media_type: ResponseMediaType) {
        if !self.is_empty() {
            append_typed_links(response, self.0, media_type);
        }
    }

    /// The target of the pointer `relation` names, when the page has one.
    #[cfg(test)]
    pub(crate) fn target(&self, relation: &RelationType) -> Option<&str> {
        self.0.iter().find(|(candidate, _)| candidate == relation).map(|(_, target)| target.as_str())
    }
}

#[cfg(test)]
mod tests {
    use crate::page_link_targets::PageLinkTargets;
    use andromeda_http_headers::{link::relation_type::RelationType, media_type::ResponseMediaType};
    use axum::{
        body::Body,
        http::{Response as HttpResponse, header::LINK},
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn a_page_pointing_nowhere_adds_no_link_header() {
        let mut response = HttpResponse::new(Body::empty());
        let targets = PageLinkTargets::default();
        assert!(targets.is_empty());
        targets.append_to(&mut response, ResponseMediaType::JsonLd);
        assert!(response.headers().get(LINK).is_none());
    }

    #[test]
    fn a_pointer_is_found_by_its_relation() {
        let targets = PageLinkTargets::new(vec![(RelationType::Next, "/ngsi-ld/v1/entities?offset=2".to_owned())]);
        assert_eq!(targets.target(&RelationType::Next), Some("/ngsi-ld/v1/entities?offset=2"));
        assert_eq!(targets.target(&RelationType::Prev), None);
    }
}
