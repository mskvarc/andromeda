//! Tenants each holding one periodic Subscription of the Device a [`TenantSet`] wrote into it, notified at the sink path
//! of the tenant's index, for a test of how a broker keeps up with the periodic work of more tenants than it keeps
//! loaded.

use crate::{broker::BrokerClient, cleanup::Cleanup, namespace::Namespace, notification_sink::NotificationSink, tenant_set::TenantSet};
use googletest::prelude::*;
use ngsi_ld::{
    model::NgsiLdMediaType,
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// How many tenants are brought up at once.
const CONCURRENCY: usize = 8;

/// Brings up `count` tenants named `<namespace>-<n>` through `broker`, each holding the Device and one Subscription of
/// it with `timeInterval` `interval_seconds`, notifying `sink` at path `n`, and returns the removals of the
/// Subscriptions, which run when the test drops them.
///
/// # Errors
///
/// Fails when a tenant cannot be brought up, a value cannot be built or a Subscription is refused.
pub async fn subscribe_periodic_tenants(
    broker: &BrokerClient,
    namespace: &Namespace,
    sink: &NotificationSink,
    count: usize,
    interval_seconds: u32,
) -> Result<Vec<Cleanup>> {
    let set = TenantSet::provision(broker, namespace, count, CONCURRENCY).await?;
    let device_type = namespace.entity_type("Device")?;
    let mut cleanups = Vec::with_capacity(count);
    for (index, member) in set.members().enumerate() {
        let subscription_id = namespace.subscription_id(index)?;
        let subscription = Subscription::builder()
            .id(subscription_id.clone()) // clone: the cleanup removes the Subscription by the id it is built with
            .entities([EntitySelector::new(SelectorTypeBuilder::from(&device_type).build()?).with_ids(set.device().clone())]) // clone: every tenant's Subscription selects the same Device
            .time_interval(interval_seconds)
            .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(index)?).accept(NgsiLdMediaType::Json).build()?).build()?)
            .build()?;
        member.client.subscriptions().create(&subscription).send().await?;
        cleanups.push(Cleanup::of(&member.client).subscription(&subscription_id));
    }
    Ok(cleanups)
}
