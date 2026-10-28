//! The notifications a committed change to current state owes, whichever API
//! made it.
//!
//! Clause 5.8.1.4 notifies "whenever there is a change in the watched
//! Attribute nodes", and clause 5.8.6 defines that change on the Entity's
//! current state alone, never on the operation that made it. A Temporal API
//! write that brings an Entity into being or advances one of its instances
//! changes current state as a Core write would, so both policies raise their
//! notifications through this one notifier: the durable matches committed with
//! the write, and the in-process change events published once it landed.

use crate::{NotificationWriteConfig, change_sink::EntityChangeSink};
use andromeda_context_commit::{change_event::ChangeEvent, context_change_event::ContextChangeEvent};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_subscription::cache::SubscriptionCache;
use std::sync::Arc;

/// Where this process matches a committed change against the subscriptions
/// it holds.
pub enum SubscriptionMatching {
    /// No subscription cache is held: nothing in this process matches a
    /// change.
    Absent,
    /// The notification worker matches every published change once it
    /// landed; the cache is read at commit only to learn whether a deletion
    /// may be owed a notification, which decides whether the state it removes
    /// is read.
    AtDelivery(Arc<SubscriptionCache>),
    /// Every match is recorded durably with the write that raised it.
    AtCommit(NotificationWriteConfig),
}

/// Raises the notifications one change to current state owes, on both
/// channels.
pub struct ChangeNotifier {
    /// Where committed changes are published in-process, absent when nothing
    /// consumes them.
    change_sink: Option<Arc<dyn EntityChangeSink>>,
    /// Where a committed change is matched against subscriptions.
    pub(crate) matching: SubscriptionMatching,
}

impl ChangeNotifier {
    /// A notifier publishing to `change_sink` and matching committed changes
    /// as `matching` says.
    ///
    /// A live-delivery broker records nothing at commit: its notification
    /// worker evaluates every published change itself.
    #[must_use]
    pub fn new(change_sink: Option<Arc<dyn EntityChangeSink>>, matching: SubscriptionMatching) -> ChangeNotifier {
        ChangeNotifier { change_sink, matching }
    }

    /// The durable matching configuration, present only on a broker recording
    /// durable deliveries.
    pub(crate) const fn recording(&self) -> Option<&NotificationWriteConfig> {
        match &self.matching {
            SubscriptionMatching::AtCommit(recording) => Some(recording),
            SubscriptionMatching::Absent | SubscriptionMatching::AtDelivery(_) => None,
        }
    }

    /// The subscription cache this process holds, whichever way it matches.
    pub(crate) const fn subscription_cache(&self) -> Option<&Arc<SubscriptionCache>> {
        match &self.matching {
            SubscriptionMatching::Absent => None,
            SubscriptionMatching::AtDelivery(subscription_cache) | SubscriptionMatching::AtCommit(NotificationWriteConfig { subscription_cache }) => {
                Some(subscription_cache)
            }
        }
    }

    /// Publish the changes one commit made, in commit order, waiting while the
    /// consumer catches up.
    pub async fn publish(&self, target: &ContextTarget, events: impl IntoIterator<Item = ChangeEvent>) {
        if let Some(ref sink) = self.change_sink {
            let changes = events
                .into_iter()
                .map(|event| ContextChangeEvent {
                    target: target.clone(), // clone: the asynchronous consumer owns the immutable branch selection.
                    event,
                })
                .collect();
            sink.publish(changes).await;
        }
    }

    /// The changes a deferred side-effect job has to carry for the
    /// notifications its commit does not queue itself.
    ///
    /// A broker recording durable deliveries commits every match with the write,
    /// so its job carries none: whichever broker drains the job, possibly one
    /// delivering live, sharing the tenant database, would otherwise notify
    /// them a second time. A live-delivery broker records none at commit, so its
    /// job carries every change and the draining broker notifies them.
    pub(crate) fn owed_change_events(&self, events: Vec<ChangeEvent>) -> Vec<ChangeEvent> {
        match self.matching {
            SubscriptionMatching::AtCommit(_) => Vec::new(),
            SubscriptionMatching::Absent | SubscriptionMatching::AtDelivery(_) => events,
        }
    }
}
