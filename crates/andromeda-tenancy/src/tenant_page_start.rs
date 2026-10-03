//! Where a page of the tenant collection starts, in tenant-name order.

use andromeda_ngsi_ld_lexicon::tenant::TenantName;

/// One page's position within the tenant collection, ordered by tenant name.
///
/// The registry lists named tenants by their unique name, a stable single
/// ordering key, so the collection can be paged by offset (clause 5.5.9.2) or
/// walked by keyset (clause 5.5.9.1) alike. The HTTP layer owns the opaque wire
/// cursor and the tenant-id URN it carries; this domain enum is what the page
/// window decodes to before it reaches the store, keeping the store working on
/// the name column it orders by and free of the URN format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TenantPageStart {
    /// Skip this many tenants, ascending by name.
    Offset(u32),
    /// A forward page: names greater than `after`, ascending. `after` is absent
    /// on the first page of a walk.
    Forward {
        /// The exclusive lower bound, or `None` for the first page.
        after: Option<TenantName>,
    },
    /// A backward page: names less than `before`, read descending and presented
    /// ascending.
    Backward {
        /// The exclusive upper bound.
        before: TenantName,
    },
    /// The final page: the greatest names, read descending and presented
    /// ascending.
    FromEnd,
}
