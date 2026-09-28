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
use std::sync::Arc;

/// Raises the notifications one change to current state owes, on both
/// channels.
pub struct ChangeNotifier {
    /// Where committed changes are published in-process, absent when nothing
    /// consumes them.
    change_sink: Option<Arc<dyn EntityChangeSink>>,
    /// Durable matching at commit time, present only on a broker recording
    /// durable deliveries.
    pub(crate) recording: Option<NotificationWriteConfig>,
}

impl ChangeNotifier {
    /// A notifier publishing to `change_sink` and recording durable matches
    /// through `recording` when given.
    ///
    /// A live-delivery broker records nothing at commit: its notification
    /// worker evaluates every published change itself.
    #[must_use]
    pub fn new(change_sink: Option<Arc<dyn EntityChangeSink>>, recording: Option<NotificationWriteConfig>) -> ChangeNotifier {
        ChangeNotifier { change_sink, recording }
    }

    /// Publish one change the store has committed.
    pub fn publish(&self, target: &ContextTarget, event: ChangeEvent) {
        if let Some(ref sink) = self.change_sink {
            sink.publish(ContextChangeEvent {
                target: target.clone(), // clone: the asynchronous consumer owns the immutable branch selection.
                event,
            });
        }
    }

    /// The changes a deferred side-effect job has to carry for the
    /// notifications its commit does not queue itself.
    ///
    /// A broker recording durable deliveries commits every match with the write,
    /// so its job carries none: whichever broker drains the job -- possibly one
    /// delivering live, sharing the tenant database -- would otherwise notify
    /// them a second time. A live-delivery broker records none at commit, so its
    /// job carries every change and the draining broker notifies them.
    pub(crate) fn owed_change_events(&self, events: Vec<ChangeEvent>) -> Vec<ChangeEvent> {
        match self.recording {
            Some(_) => Vec::new(),
            None => events,
        }
    }
}
