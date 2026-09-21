//! One page of the tenant collection.

use crate::tenant_view::TenantView;

/// A page of tenant views together with the total number of named tenants.
///
/// The HTTP layer turns the views into tenant resources and derives the page's
/// cursor links from the first and last view; the total feeds the count header.
#[derive(Debug, Clone)]
pub struct TenantListing {
    /// The tenant views on this page, in ascending name order.
    pub views: Vec<TenantView>,
    /// The total number of named tenants in the registry.
    pub total: i64,
}

impl TenantListing {
    /// Assemble a listing from its page of views and the collection total.
    #[must_use]
    pub const fn new(views: Vec<TenantView>, total: i64) -> TenantListing {
        TenantListing { views, total }
    }
}
