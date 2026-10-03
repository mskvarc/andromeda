//! One page of a subscription collection, ready for a response (clauses 5.8.4
//! and 5.11.4).

use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use serde_json::Value;

/// A page of subscriptions, ready for a collection response.
///
/// Carries the rendered resources alongside the two things the clause 6.3.10
/// `Link` builder needs that the rendered bodies no longer expose: the total for
/// the optional clause 6.3.13 count, and the identifiers of the first and last
/// resource on the page, from which the neighbouring cursors (clause 5.5.9.1)
/// are computed.
pub struct SubscriptionListing {
    /// The subscriptions the page carries, serialized for the response and in
    /// ascending identifier order.
    pub subscriptions: Vec<Value>,
    /// The total number of subscriptions of this kind (clause 6.3.13), reported
    /// when the request asked for a count.
    pub total: i64,
    /// The first and last identifiers of the page as displayed; `None` when the
    /// page is empty and so has no boundary to walk from.
    pub boundary: Option<(Identifier, Identifier)>,
}
