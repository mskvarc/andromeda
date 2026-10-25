//! Parameters for listing hosted and cached Contexts.

use crate::detail_level::ContextDetailLevel;
use andromeda_ngsi_ld_model::jsonld_context::ContextKind;
use andromeda_ngsi_ld_query::page_window::PageWindow;

/// One page of the Context listing, in local-identifier order.
#[derive(Debug)]
pub struct ContextListQuery<'a> {
    /// Optional Context-kind filter, applied to the stored rows.
    pub kind: Option<&'a ContextKind>,
    /// Requested response projection.
    pub detail_level: ContextDetailLevel,
    /// The page the pagination policy decided (clause 5.5.9).
    pub window: &'a PageWindow,
}

#[cfg(test)]
mod tests {
    use crate::{detail_level::ContextDetailLevel, list_query::ContextListQuery};
    use andromeda_ngsi_ld_query::page_window::PageWindow;

    #[test]
    fn query_can_request_an_unfiltered_summary_page() {
        let window = PageWindow::offset(10, 0);
        let query = ContextListQuery {
            kind: None,
            detail_level: ContextDetailLevel::Summary,
            window: &window,
        };
        pretty_assertions::assert_eq!(query.window.limit, 10);
    }
}
