//! The members of a Subscription whose data type is a URI.
//!
//! Clause 4.5.0 spells the NGSI-LD Null as the URI `urn:ngsi-ld:null` "in all
//! places where URI strings are valid JSON values", so these are the members a
//! request can write the Null into as a URI. Clause 5.5.8 gives it the meaning
//! "the target member is deleted" in an update, and clause 5.2.1 makes it
//! `BadRequestData` anywhere else, a create among them.

use strum::Display;

/// A member of a Subscription that takes a URI (Table 5.2.12-1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Display)]
#[strum(serialize_all = "camelCase")]
pub enum SubscriptionUriMember {
    /// `jsonldContext`, the `@context` notifications are rendered with.
    JsonldContext,
    /// `datasetId`, the dataset identifiers a notification carries.
    DatasetId,
    /// `watchedAttributes`, the attribute names a subscription watches, which
    /// are URIs once expanded.
    WatchedAttributes,
}

#[cfg(test)]
mod tests {
    use crate::subscription_uri_member::SubscriptionUriMember;

    /// The members are named as Table 5.2.12-1 spells them.
    #[test]
    fn a_member_is_named_as_the_specification_spells_it() {
        pretty_assertions::assert_eq!(SubscriptionUriMember::JsonldContext.to_string(), "jsonldContext");
        pretty_assertions::assert_eq!(SubscriptionUriMember::DatasetId.to_string(), "datasetId");
        pretty_assertions::assert_eq!(SubscriptionUriMember::WatchedAttributes.to_string(), "watchedAttributes");
    }
}
