//! Whether an update fragment replaces the stored members the broker cannot read.
//!
//! A member the store lists as unreadable is held as absent, so merging a fragment over it and writing the result back would store nothing for it: a subscription that selected nothing would select more than before. An update may only leave such a member when it replaces it.

use crate::{expanded_terms::ExpandedTerms, subscription_invariant::SubscriptionInvariant};
use andromeda_ngsi_ld_model::{subscription::unreadable_member::UnreadableMember, subscription_request::SubscriptionRequest};

/// Check that the update fragment `request`, whose names are `expanded`, replaces every member of `unreadable`.
///
/// A member is replaced when the fragment sets it or deletes it (clause 5.5.8), and the notification parameters are replaced whole when the fragment names them (clause 5.8.3).
///
/// # Errors
///
/// Returns [`SubscriptionInvariant::UnreadableMemberKept`] for the first member the fragment leaves as stored.
pub(crate) fn ensure_replaced(unreadable: &[UnreadableMember], expanded: &ExpandedTerms, request: &SubscriptionRequest) -> Result<(), SubscriptionInvariant> {
    unreadable.iter().try_for_each(|member| {
        let replaced = match member {
            UnreadableMember::Entities => !expanded.entities.is_absent(),
            UnreadableMember::WatchedAttributes => !expanded.watched_attributes.is_absent(),
            UnreadableMember::Q => !expanded.q.is_absent(),
            UnreadableMember::Csf => !expanded.csf.is_absent(),
            UnreadableMember::GeoQ => !expanded.geo_q.is_absent(),
            UnreadableMember::ExpandValues => !expanded.expand_values.is_absent(),
            UnreadableMember::JsonKeys => !expanded.json_keys.is_absent(),
            UnreadableMember::NotificationAttributes | UnreadableMember::Pick | UnreadableMember::Omit => request.notification.is_some(),
        };
        if replaced {
            Ok(())
        } else {
            Err(SubscriptionInvariant::UnreadableMemberKept(*member))
        }
    })
}

#[cfg(test)]
mod tests {
    use crate::{expanded_terms::ExpandedTerms, subscription_invariant::SubscriptionInvariant, unreadable_replacement::ensure_replaced};
    use andromeda_ngsi_ld_model::{
        clearable::Clearable,
        request_body::RequestBody,
        subscription::unreadable_member::UnreadableMember,
        subscription_request::SubscriptionRequest,
    };
    use andromeda_ngsi_ld_representation::compact_node::context_reading::ContextReading;
    use strum::IntoEnumIterator;

    fn request(wire: &str) -> SubscriptionRequest {
        SubscriptionRequest::from_request_text(wire.as_bytes(), ContextReading::keyword_spellings()).expect("a readable request")
    }

    fn deleting_every_selector() -> ExpandedTerms {
        ExpandedTerms {
            entities: Clearable::NgsiLdNull,
            watched_attributes: Clearable::NgsiLdNull,
            q: Clearable::NgsiLdNull,
            geo_q: Clearable::NgsiLdNull,
            csf: Clearable::NgsiLdNull,
            expand_values: Clearable::NgsiLdNull,
            json_keys: Clearable::NgsiLdNull,
            ..ExpandedTerms::default()
        }
    }

    #[test]
    fn a_fragment_naming_none_of_the_unreadable_members_keeps_the_first_of_them() {
        let refused = ensure_replaced(&[UnreadableMember::Q, UnreadableMember::Csf], &ExpandedTerms::default(), &request("{}"));

        assert!(
            matches!(refused, Err(SubscriptionInvariant::UnreadableMemberKept(UnreadableMember::Q))),
            "{refused:?}"
        );
    }

    #[test]
    fn a_fragment_that_sets_or_deletes_the_unreadable_member_replaces_it() {
        let set = ExpandedTerms {
            watched_attributes: Clearable::Set(Vec::new()),
            ..ExpandedTerms::default()
        };
        assert!(ensure_replaced(&[UnreadableMember::WatchedAttributes], &set, &request("{}")).is_ok());

        let deleted = ExpandedTerms {
            q: Clearable::NgsiLdNull,
            ..ExpandedTerms::default()
        };
        assert!(ensure_replaced(&[UnreadableMember::Q], &deleted, &request("{}")).is_ok());
    }

    #[test]
    fn the_notification_members_are_replaced_only_by_a_fragment_that_names_the_notification() {
        let listed = UnreadableMember::iter().collect::<Vec<_>>();
        let terms = deleting_every_selector();

        let kept = ensure_replaced(&listed, &terms, &request("{}"));
        assert!(
            matches!(kept, Err(SubscriptionInvariant::UnreadableMemberKept(UnreadableMember::NotificationAttributes))),
            "{kept:?}"
        );

        let named = request(r#"{"notification": {"endpoint": {"uri": "https://receiver.example.com/notify"}}}"#);
        assert!(ensure_replaced(&listed, &terms, &named).is_ok());
    }
}
