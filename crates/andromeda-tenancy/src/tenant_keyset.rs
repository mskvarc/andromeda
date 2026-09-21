//! Where a page of the tenant collection starts, in tenant-name order.

use andromeda_ngsi_ld_lexicon::tenant::TenantName;

/// One page's position within the tenant collection, ordered by tenant name.
///
/// The registry lists named tenants by their unique name, which is a stable
/// single ordering key, so the collection is walked by keyset rather than offset
/// (clause 5.5.9.1). The HTTP layer owns the opaque wire cursor and the tenant-id
/// URN it carries; this domain enum is what that cursor decodes to before it
/// reaches the store, keeping the store working on the name column it orders by
/// and free of the URN format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TenantKeyset {
    /// A forward page: names greater than `after`, ascending. `after` is absent
    /// on the first page.
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

impl TenantKeyset {
    /// The keyset for the first page of the collection.
    #[must_use]
    pub const fn first_page() -> TenantKeyset {
        TenantKeyset::Forward { after: None }
    }
}
