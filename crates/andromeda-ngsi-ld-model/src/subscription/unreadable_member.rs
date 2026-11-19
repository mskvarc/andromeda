//! The stored members of a subscription that cannot be read back in the expanded stage.

use strum::{Display, EnumIter, IntoStaticStr};

/// A stored member whose text is no valid expanded form, so the subscription holds nothing for it.
///
/// A name that is no absolute IRI selects no entity of an expanded store, so a subscription with an unreadable member selects nothing until it is updated. A member is never kept in part: a list that loses a name widens what the subscription selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, EnumIter, IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum UnreadableMember {
    /// The `entities` selectors.
    Entities,
    /// The `watchedAttributes` names.
    WatchedAttributes,
    /// The `q` filter.
    Q,
    /// The context source filter.
    Csf,
    /// The `geoQ` geo-query.
    GeoQ,
    /// The `expandValues` names.
    ExpandValues,
    /// The `jsonKeys` names.
    JsonKeys,
    /// The `attributes` of the notification.
    NotificationAttributes,
    /// The `pick` projections of the notification.
    Pick,
    /// The `omit` projections of the notification.
    Omit,
}
