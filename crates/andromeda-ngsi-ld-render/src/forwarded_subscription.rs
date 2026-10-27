//! The reduced Subscription forwarded to a context source (clauses 4.3.6.6 and
//! 5.8.1.4), its deletions written as the source's version reads them.
//!
//! An update of the copy at the source deletes, with an NGSI-LD Null, each
//! member the local update deleted (clauses 5.5.8 and 5.8.2.4). Every member of
//! a Subscription Fragment is merged by clause 5.5.8, so a source up to 1.4
//! reads the deletion as JSON `null`, a 1.5 source reads none and is sent the
//! fragment without it, and a later one reads the Null as written. A create
//! deletes nothing and is written as it is.

use crate::version::deletion::{DeletionEncoding, DeletionSite};
use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;
use andromeda_ngsi_ld_model::{subscription_member::SubscriptionMember, subscription_request::SubscriptionRequest};
use serde::{Serialize, Serializer};

/// A Subscription create body or update fragment, as one source reads it.
#[derive(Debug, Clone, Serialize)]
pub struct ForwardedSubscription {
    /// The members written as the request holds them.
    #[serde(flatten)]
    request: SubscriptionRequest,
    /// The members deleted by JSON `null`, left out of `request`.
    #[serde(flatten, serialize_with = "json_null_members")]
    json_null: Vec<SubscriptionMember>,
}

impl ForwardedSubscription {
    /// `request` written for a source that conforms to `conformance`, when the
    /// registration names a version.
    #[must_use]
    pub fn new(request: SubscriptionRequest, conformance: Option<SpecVersion>) -> ForwardedSubscription {
        match DeletionEncoding::for_target(conformance, DeletionSite::MergedMember) {
            Some(DeletionEncoding::NgsiLdNull) => ForwardedSubscription {
                request,
                json_null: Vec::new(),
            },
            Some(DeletionEncoding::JsonNull) => ForwardedSubscription {
                json_null: request.deleted_members().collect(),
                request: request.without_deletions(),
            },
            None => ForwardedSubscription {
                request: request.without_deletions(),
                json_null: Vec::new(),
            },
        }
    }
}

/// Write each of `members` with JSON `null` as its value.
fn json_null_members<S: Serializer>(members: &[SubscriptionMember], serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_map(members.iter().map(|member| (<&'static str>::from(*member), ())))
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::forwarded_subscription::ForwardedSubscription;
    use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;
    use andromeda_ngsi_ld_model::subscription_request::SubscriptionRequest;
    use serde_json::{Value, json};

    /// An update fragment deleting `entities`, `q` and `expiresAt` while it
    /// sets `watchedAttributes` and the notification endpoint.
    fn fragment() -> SubscriptionRequest {
        serde_json::from_value(json!({
            "entities": ["urn:ngsi-ld:null"],
            "watchedAttributes": ["speed"],
            "q": "urn:ngsi-ld:null",
            "expiresAt": "urn:ngsi-ld:null",
            "notification": {"endpoint": {"uri": "http://broker.example.org/notify"}}
        }))
        .expect("a subscription fragment")
    }

    fn forwarded(conformance: Option<SpecVersion>) -> Value {
        serde_json::to_value(ForwardedSubscription::new(fragment(), conformance)).expect("the fragment writes")
    }

    /// Clause 4.5.0: a source of 1.6 or later, or naming no version, reads the
    /// NGSI-LD Null, an array holding it alone for a member that takes one.
    #[test]
    fn a_source_from_1_6_receives_the_ngsi_ld_null() {
        for conformance in [None, Some(SpecVersion::V1_6), Some(SpecVersion::V1_8)] {
            pretty_assertions::assert_eq!(
                forwarded(conformance),
                json!({
                    "entities": ["urn:ngsi-ld:null"],
                    "watchedAttributes": ["speed"],
                    "q": "urn:ngsi-ld:null",
                    "expiresAt": "urn:ngsi-ld:null",
                    "notification": {"endpoint": {"uri": "http://broker.example.org/notify"}}
                }),
                "{conformance:?}"
            );
        }
    }

    /// V1.4.2 clauses 5.5.8 and 5.8.2.4: a Subscription member is removed by
    /// JSON `null`, whatever its data type.
    #[test]
    fn a_1_4_source_receives_each_deletion_as_json_null() {
        pretty_assertions::assert_eq!(
            forwarded(Some(SpecVersion::V1_4)),
            json!({
                "entities": null,
                "watchedAttributes": ["speed"],
                "q": null,
                "expiresAt": null,
                "notification": {"endpoint": {"uri": "http://broker.example.org/notify"}}
            })
        );
    }

    /// V1.5.1 clause 5.5.4 refuses any null member value and its clause 5.5.8
    /// deletes nothing, so the deletions are left out and the rest travels.
    #[test]
    fn a_1_5_source_receives_no_deletion() {
        let written = forwarded(Some(SpecVersion::V1_5));
        pretty_assertions::assert_eq!(
            written,
            json!({
                "watchedAttributes": ["speed"],
                "notification": {"endpoint": {"uri": "http://broker.example.org/notify"}}
            })
        );
        assert!(!written.to_string().contains("null"), "{written}");
    }
}
