//! The in-process change channel a live-delivery side-effect drain publishes
//! to, recorded so a verification can read what the drain would notify.

use andromeda_context_commit::context_change_event::ContextChangeEvent;
use andromeda_entity::change_sink::EntityChangeSink;
use std::{mem, sync::Mutex};

/// Every change a drain published, in publication order.
#[derive(Debug, Default)]
pub struct RecordedChanges {
    /// The published changes.
    published: Mutex<Vec<ContextChangeEvent>>,
}

impl RecordedChanges {
    /// The changes published so far, taken out of the record.
    pub fn take(&self) -> Vec<ContextChangeEvent> {
        mem::take(&mut *self.published.lock().expect("published changes lock"))
    }
}

impl EntityChangeSink for RecordedChanges {
    fn publish(&self, event: ContextChangeEvent) {
        self.published.lock().expect("published changes lock").push(event);
    }
}
