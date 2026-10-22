//! The fields of a subscription carrying nothing optional, for tests of the
//! statements that write them.

use andromeda_subscription::store::SubscriptionFields;

/// Fields with every optional member absent.
pub(super) const fn bare_fields<'a>() -> SubscriptionFields<'a> {
    SubscriptionFields {
        subscription_name: None,
        description: None,
        entities: None,
        watched_attributes: None,
        notification_trigger: None,
        time_interval: None,
        q: None,
        geo_q: None,
        scope_q: None,
        csf: None,
        dataset_id: None,
        is_active: true,
        notification: None,
        expires_at: None,
        throttling: None,
        temporal_q: None,
        expand_values: None,
        json_keys: None,
        lang: None,
        jsonld_context: None,
        local_only: false,
        split_entities: None,
        ngsild_conformance: None,
    }
}
