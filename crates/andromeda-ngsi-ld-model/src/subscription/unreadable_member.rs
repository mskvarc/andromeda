//! The stored members of a subscription that cannot be read back in the expanded stage.

use strum::{Display, EnumIter, IntoStaticStr};

/// A stored member whose text is no valid expanded form, so the subscription holds nothing for it.
///
/// A name that is no absolute IRI selects no entity of an expanded store, so a subscription with an unreadable member selects nothing until it is updated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, EnumIter, IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum UnreadableMember {
    /// The `entities` selectors.
    Entities,
    /// The `q` filter.
    Q,
    /// The context source filter.
    Csf,
    /// The `geoQ` geo-query.
    GeoQ,
}
